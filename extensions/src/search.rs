use actix::{Actor, ActorFutureExt, AsyncContext, WrapFuture};
use nostr_relay::{
    message::{ClientMessage, IncomingMessage, OutgoingMessage},
    setting::SettingWrapper,
    Extension, ExtensionMessageResult, Session,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tracing::warn;

fn default_namespace() -> String {
    "nostr".to_owned()
}
fn default_timeout_ms() -> u64 {
    10_000
}

/// NIP-50 search configuration.
///
/// Without `service_url`, behavior is unchanged: rnostr's local keyword index
/// handles search. With a URL, committed events are mirrored to the service and
/// NIP-50 REQs are answered by it. The service itself is generic; only its
/// `/v1/adapters/nostr/*` adapter is used here.
#[derive(Deserialize, Debug)]
#[serde(default)]
pub struct SearchSetting {
    pub enabled: bool,
    pub service_url: Option<String>,
    pub service_token: Option<String>,
    pub namespace: String,
    pub timeout_ms: u64,
}

impl Default for SearchSetting {
    fn default() -> Self {
        Self {
            enabled: false,
            service_url: None,
            service_token: None,
            namespace: default_namespace(),
            timeout_ms: default_timeout_ms(),
        }
    }
}

#[derive(Debug)]
pub struct Search {
    setting: SearchSetting,
    client: reqwest::Client,
}

impl Default for Search {
    fn default() -> Self {
        Self { setting: SearchSetting::default(), client: reqwest::Client::new() }
    }
}

#[derive(Deserialize)]
struct RemoteHit {
    event: Value,
}

#[derive(Deserialize)]
struct RemoteSearchResponse {
    results: Vec<RemoteHit>,
}

impl Search {
    pub fn new() -> Self {
        Self::default()
    }

    fn endpoint(&self, path: &str) -> Option<String> {
        self.setting.service_url.as_ref().map(|u| {
            format!("{}{}", u.trim_end_matches('/'), path)
        })
    }

    fn request(&self, method: reqwest::Method, url: String) -> reqwest::RequestBuilder {
        let req = self.client.request(method, url)
            .timeout(Duration::from_millis(self.setting.timeout_ms));
        if let Some(token) = &self.setting.service_token {
            req.bearer_auth(token)
        } else {
            req
        }
    }
}

impl Extension for Search {
    fn name(&self) -> &'static str {
        "search"
    }

    fn setting(&mut self, setting: &SettingWrapper) {
        let mut w = setting.write();
        self.setting = w.parse_extension(self.name());
        if self.setting.enabled {
            w.add_nip(50);
        }
    }

    fn message(
        &self,
        mut msg: ClientMessage,
        session: &mut Session,
        ctx: &mut <Session as Actor>::Context,
    ) -> ExtensionMessageResult {
        if !self.setting.enabled {
            return ExtensionMessageResult::Continue(msg);
        }

        match &mut msg.msg {
            IncomingMessage::Event(event) => {
                // Keep local NIP-50 indexing behavior. Remote ingest happens in
                // event_written(), after the event is actually committed.
                event.build_note_words();
            }
            IncomingMessage::Req(sub) => {
                for filter in &mut sub.filters {
                    filter.build_words();
                }

                let Some(url) = self.endpoint("/v1/adapters/nostr/search") else {
                    return ExtensionMessageResult::Continue(msg);
                };
                let Some(filter) = sub.filters.iter().find(|f| f.search.is_some()) else {
                    return ExtensionMessageResult::Continue(msg);
                };
                let query = filter.search.clone().unwrap_or_default();
                let sub_id = sub.id.clone();
                let limit = filter.limit.unwrap_or(20).min(500);

                // Preserve the original NIP-01/50 filter JSON rather than
                // teaching the generic service about rnostr's Rust Filter type.
                let filter_json = serde_json::from_str::<Value>(&msg.text)
                    .ok()
                    .and_then(|v| v.as_array().cloned())
                    .and_then(|a| a.into_iter().skip(2).find(|v| {
                        v.get("search").and_then(Value::as_str).is_some()
                    }))
                    .unwrap_or_else(|| json!({"search": query, "limit": limit}));
                let body = json!({
                    "query": query,
                    "filter": filter_json,
                    "limit": limit,
                    "namespace": self.setting.namespace,
                });
                let req = self.request(reqwest::Method::POST, url).json(&body);
                let future = async move {
                    let response = req.send().await.map_err(|e| e.to_string())?;
                    if !response.status().is_success() {
                        return Err(format!("memory service HTTP {}", response.status()));
                    }
                    response.json::<RemoteSearchResponse>().await.map_err(|e| e.to_string())
                };
                ctx.spawn(future.into_actor(session).map(move |result, _session, ctx| {
                    match result {
                        Ok(response) => {
                            for hit in response.results {
                                ctx.text(OutgoingMessage::event(&sub_id, &hit.event.to_string()));
                            }
                            ctx.text(OutgoingMessage::eose(&sub_id));
                        }
                        Err(err) => {
                            ctx.text(OutgoingMessage::closed(
                                &sub_id, &format!("remote-search: {}", err),
                            ));
                        }
                    }
                }));
                return ExtensionMessageResult::Ignore;
            }
            _ => {}
        }
        ExtensionMessageResult::Continue(msg)
    }

    fn event_written(&self, event: &nostr_relay::db::Event) {
        let Some(url) = self.endpoint("/v1/adapters/nostr/events") else {
            return;
        };
        let Ok(value) = serde_json::from_str::<Value>(&event.to_string()) else {
            warn!("remote search: failed to serialize committed event");
            return;
        };
        let body = json!({"event": value, "namespace": self.setting.namespace});
        let req = self.request(reqwest::Method::POST, url).json(&body);
        actix::spawn(async move {
            match req.send().await {
                Ok(response) if response.status().is_success() => {}
                Ok(response) => warn!(status = %response.status(), "remote search ingest rejected"),
                Err(err) => warn!(error = %err, "remote search ingest failed"),
            }
        });
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::create_test_app;
    use actix_web::web;
    use actix_web_actors::ws;
    use anyhow::Result;
    use futures_util::{SinkExt as _, StreamExt as _};
    use nostr_relay::create_web_app;
    use nostr_relay::db::{
        now,
        secp256k1::{rand::thread_rng, Keypair},
        Event,
    };

    fn parse_text<T: serde::de::DeserializeOwned>(frame: &ws::Frame) -> Result<T> {
        if let ws::Frame::Text(text) = &frame {
            // println!("message: {:?}", String::from_utf8(text.to_vec()));
            let data: T = serde_json::from_slice(text)?;
            Ok(data)
        } else {
            Err(nostr_relay::Error::Message("invalid frame type".to_string()).into())
        }
    }

    #[actix_rt::test]
    async fn message() -> Result<()> {
        let mut rng = thread_rng();
        let key_pair = Keypair::new_global(&mut rng);

        let app = create_test_app("search")?;
        {
            let mut w = app.setting.write();
            w.extra = serde_json::from_str(
                r#"{
                "search": {
                    "enabled": true
                }
            }"#,
            )?;
        }

        let app = app.add_extension(Search::new());
        let app = web::Data::new(app);

        let mut srv = actix_test::start(move || create_web_app(app.clone()));

        // client service
        let mut framed = srv.ws_at("/").await.unwrap();

        let start = now();
        // write
        for (index, content) in ["test", "来自中国的nostr用户", "nostr users from China"]
            .into_iter()
            .enumerate()
        {
            let event = Event::create(
                &key_pair,
                start + index as u64,
                1,
                vec![],
                content.to_owned(),
            )?;
            let msg = format!(r#"["EVENT", {}]"#, event.to_string());
            framed.send(ws::Message::Text(msg.into())).await?;
            let notice: (String, String, bool, String) =
                parse_text(&framed.next().await.unwrap()?)?;
            assert!(notice.2);
        }

        // get
        framed
            .send(ws::Message::Text(
                r#"["REQ", "1", {"search": "nostr"}]"#.into(),
            ))
            .await?;
        let res: (String, String, Event) = parse_text(&framed.next().await.unwrap()?)?;
        assert_eq!(res.2.content(), "来自中国的nostr用户");
        let res: (String, String, Event) = parse_text(&framed.next().await.unwrap()?)?;
        assert_eq!(res.2.content(), "nostr users from China");
        let res: (String, String) = parse_text(&framed.next().await.unwrap()?)?;
        assert_eq!(res.0, "EOSE");

        framed
            .send(ws::Message::Text(
                r#"["REQ", "2", {"search": "中国nostr"}]"#.into(),
            ))
            .await?;
        let res: (String, String, Event) = parse_text(&framed.next().await.unwrap()?)?;
        assert_eq!(res.2.content(), "来自中国的nostr用户");
        let res: (String, String) = parse_text(&framed.next().await.unwrap()?)?;
        assert_eq!(res.0, "EOSE");

        framed
            .send(ws::Message::Text(
                r#"["REQ", "3", {"search": "china nostr"}]"#.into(),
            ))
            .await?;
        let res: (String, String, Event) = parse_text(&framed.next().await.unwrap()?)?;
        assert_eq!(res.2.content(), "nostr users from China");
        let res: (String, String) = parse_text(&framed.next().await.unwrap()?)?;
        assert_eq!(res.0, "EOSE");

        // close
        framed
            .send(ws::Message::Close(Some(ws::CloseCode::Normal.into())))
            .await?;
        let item = framed.next().await.unwrap()?;
        assert_eq!(item, ws::Frame::Close(Some(ws::CloseCode::Normal.into())));

        Ok(())
    }
}
