use nostr_db::SortList;
use nostr_relay::{
    message::{ClientMessage, IncomingMessage},
    setting::SettingWrapper,
    Extension, ExtensionMessageResult, Session,
};

use serde::Deserialize;

use crate::search_client::{EventSearchRequest, ExternalSearchClient};

#[derive(Deserialize, Default, Debug)]
pub struct ExternalSearchSetting {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub timeout: Option<String>,
    pub fallback_to_local: Option<bool>,
    pub auth_token: Option<String>,
    pub max_results: Option<u32>,
}

#[derive(Default, Debug)]
pub struct ExternalSearch {
    setting: ExternalSearchSetting,
    client: Option<ExternalSearchClient>,
}

impl ExternalSearch {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Extension for ExternalSearch {
    fn name(&self) -> &'static str {
        "external_search"
    }

    fn setting(&mut self, setting: &SettingWrapper) {
        let w = setting.read();
        self.setting = w.parse_extension(self.name());

        if self.setting.enabled {
            if let Some(endpoint) = &self.setting.endpoint {
                let timeout = self
                    .setting
                    .timeout
                    .as_ref()
                    .and_then(|t| t.parse::<u64>().ok());

                self.client = Some(ExternalSearchClient::new(endpoint.clone(), timeout));
            } else {
                println!("⚠️ External search enabled but no endpoint configured");
            }
        } else {
            self.client = None;
        }
    }

    fn message(
        &self,
        mut msg: ClientMessage,
        _session: &mut Session,
        _ctx: &mut <Session as actix::Actor>::Context,
    ) -> ExtensionMessageResult {
        if self.setting.enabled && self.client.is_some() {
            match &mut msg.msg {
                IncomingMessage::Event(event) => {
                    event.build_note_words();
                    if let Some(client) = &self.client {
                        client.post_event_blocking(&event).unwrap();
                    }
                }
                IncomingMessage::Req(ref mut subscription) => {
                    for filter in &mut subscription.filters {
                        if let Some(search_text) = &filter.search {
                            if !search_text.is_empty() {
                                if let Some(client) = &self.client {
                                    let search_request = EventSearchRequest {
                                        search: Some(search_text.clone()),
                                        limit: self.setting.max_results.map(|x| x as usize),
                                        kinds: None,
                                        authors: None,
                                        since: None,
                                        until: None,
                                        ids: None,
                                    };

                                    match client.search_events_blocking(&search_request) {
                                        Ok(search_response) => {
                                            if !search_response.event_ids.is_empty() {
                                                println!(
                                                    "External search found {} events: {:?}",
                                                    search_response.event_ids.len(),
                                                    search_response
                                                );
                                                let mut event_id_arrays = Vec::new();
                                                for event_id_hex in &search_response.event_ids {
                                                    if let Ok(event_id_bytes) =
                                                        hex::decode(event_id_hex)
                                                    {
                                                        if let Ok(event_id_array) =
                                                            event_id_bytes.try_into()
                                                        {
                                                            event_id_arrays.push(event_id_array);
                                                        }
                                                    }
                                                }

                                                if !event_id_arrays.is_empty() {
                                                    println!("Converted {} external event IDs for local lookup", event_id_arrays.len());
                                                    filter.search = None;
                                                    // Reverse order from external search to get original relevance order
                                                    event_id_arrays.reverse();
                                                    filter.ids = nostr_db::SortList::new_unsorted(
                                                        event_id_arrays,
                                                    );
                                                    filter.authors = SortList::from(vec![]);
                                                    filter.kinds = SortList::from(vec![]);
                                                    filter.since = None;
                                                    filter.until = None;
                                                    filter.tags.clear();
                                                    filter.limit = Some(200);
                                                    filter.desc = false;
                                                }
                                            } else {
                                                println!("External search returned no results, and fallback_to_local is disabled");
                                                // Clear search to return no results
                                                filter.search = None;
                                                filter.ids = SortList::from(vec![]);
                                                filter.limit = Some(0);
                                            }
                                        }
                                        Err(e) => {
                                            println!("External search failed: {:?}", e);
                                            if self.setting.fallback_to_local.unwrap_or(true) {
                                                println!("Falling back to local search due to external search error");
                                                // Keep the original search filter for local search
                                            } else {
                                                println!("Fallback to local search is disabled, clearing search");
                                                // Clear search to return no results
                                                filter.search = None;
                                                filter.ids = SortList::from(vec![]);
                                                filter.limit = Some(0);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        ExtensionMessageResult::Continue(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_external_search_extension() {
        let ext = ExternalSearch::new();

        // Test with disabled setting
        assert!(!ext.setting.enabled);
        assert!(ext.client.is_none());

        // Test name
        assert_eq!(ext.name(), "external_search");
    }
}
