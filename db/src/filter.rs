use crate::{error::Error, ArchivedEventIndex, EventIndex};
use serde::Deserialize;
use serde_json::Value;
use std::cmp::Ord;
use std::{collections::HashMap, ops::Deref, str::FromStr};

/// The sort list contains unduplicated and sorted items
#[derive(PartialEq, Eq, Debug, Clone, Default)]
pub struct SortList<T>(Vec<T>);

impl<T: Ord> From<Vec<T>> for SortList<T> {
    fn from(mut value: Vec<T>) -> Self {
        value.sort();
        value.dedup();
        Self(value)
    }
}

impl<T: Ord> SortList<T> {
    /// Create a SortList preserving the original order (for external search results)
    pub fn new_unsorted(value: Vec<T>) -> Self {
        Self(value)
    }

    pub fn contains(&self, item: &T) -> bool {
        self.binary_search(item).is_ok()
    }
}

impl<T: Ord + AsRef<[u8]>> SortList<T> {
    pub fn contains2<I: AsRef<[u8]>>(&self, item: I) -> bool {
        self.binary_search_by(|p| p.as_ref().cmp(item.as_ref()))
            .is_ok()
    }
}

impl<T> Deref for SortList<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Events filter
///
/// [NIP-01](https://nips.be/1)

// TODO: hashset uniq, (default limit), limit length, limit item length, empty string, invald hex prefix, validate length
#[derive(PartialEq, Eq, Debug, Clone, Default, Deserialize)]
#[serde(try_from = "_Filter")]
pub struct Filter {
    /// a list of event ids
    pub ids: SortList<[u8; 32]>,

    /// a list of pubkeys, the pubkey of an event must be one of these
    pub authors: SortList<[u8; 32]>,

    /// a list of a kind numbers
    pub kinds: SortList<u16>,

    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: Option<u64>,

    /// Keyword search  [NIP-50](https://nips.be/50) , [keywords renamed to search](https://github.com/nostr-protocol/nips/commit/6708a73bbcd141094c75f739c8b31446620b30e1)
    pub search: Option<String>,

    /// tags starts with "#", key tag length 1
    ///
    pub tags: HashMap<Vec<u8>, SortList<Vec<u8>>>,

    /// Query by time descending order
    pub desc: bool,

    #[serde(skip)]
    pub words: Vec<Vec<u8>>,
}

impl FromStr for Filter {
    type Err = serde_json::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct _Filter {
    pub ids: Vec<_HexString>,
    pub authors: Vec<_HexString>,
    pub kinds: Vec<u16>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: Option<u64>,
    pub keywords: Vec<String>,
    pub search: Option<String>,
    #[serde(flatten)]
    pub tags: HashMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(transparent)]
struct _HexString {
    #[serde(with = "hex::serde")]
    hex: [u8; 32],
}

impl TryFrom<_Filter> for Filter {
    type Error = Error;
    fn try_from(filter: _Filter) -> Result<Self, Self::Error> {
        // deserialize search option, convert keywords array to string
        let mut search = filter.search;
        if search.is_none() && !filter.keywords.is_empty() {
            search = Some(filter.keywords.join(" "));
        }

        // Handle tags and geohash prefix queries
        let mut tags = HashMap::new();
        for item in filter.tags {
            let key = item.0;

            // Handle geohash prefix queries (#g^ and #g^N)
            if key == "#g^" || (key.starts_with("#g^") && key.len() > 3 && key[3..].parse::<u8>().is_ok()) {
                // Convert geohash prefix to special g-tag entries
                let values = if key == "#g^" {
                    // #g^ = prefix search without length constraint
                    let val = Vec::<String>::deserialize(&item.1)?;
                    val.into_iter().map(|prefix| format!("^{}", prefix)).collect::<Vec<_>>()
                } else {
                    // #g^N = prefix search with max length N
                    let max_len = key[3..].parse::<u8>().unwrap();
                    let val = Vec::<String>::deserialize(&item.1)?;
                    val.into_iter().map(|prefix| format!("^{}:{}", prefix, max_len)).collect::<Vec<_>>()
                };

                if !values.is_empty() {
                    let list = values.into_iter().map(|s| s.into_bytes()).collect::<Vec<_>>();
                    tags.insert(b"g".to_vec(), list.into());
                }
            } else if let Some(key) = key.strip_prefix('#') {
                let key = key.as_bytes();
                // only index for key len 1
                if key.len() == 1 {
                    let val = Vec::<String>::deserialize(&item.1)?;
                    let mut list = vec![];
                    for s in val {
                        if key == b"e" || key == b"p" {
                            let h = hex::decode(&s)?;
                            if h.len() != 32 {
                                // ignore
                                return Err(Error::Invalid("invalid e or p tag value".to_string()));
                            } else {
                                list.push(h);
                            }
                        } else {
                            list.push(s.into_bytes());
                            // if s.len() < 255 {
                            // } else {
                            //     return Err(Error::Invald("invalid value length".to_string()));
                            // }
                        }
                    }
                    if !list.is_empty() {
                        tags.insert(key.to_vec(), list.into());
                    }
                }
            }
        }

        let f = Filter {
            ids: filter
                .ids
                .into_iter()
                .map(|s| s.hex)
                .collect::<Vec<_>>()
                .into(),
            authors: filter
                .authors
                .into_iter()
                .map(|s| s.hex)
                .collect::<Vec<_>>()
                .into(),
            kinds: filter.kinds.into(),
            since: filter.since,
            until: filter.until,
            limit: filter.limit,
            search,
            tags,
            desc: filter.limit.is_some(),
            words: vec![],
        };

        Ok(f)
    }
}

impl Filter {
    #[cfg(feature = "search")]
    /// build keywords for search ability
    pub fn build_words(&mut self) {
        if let Some(search) = &self.search {
            let words = crate::segment(search);
            if !words.is_empty() {
                self.words = words;
            }
        }
    }

    pub fn default_limit(&mut self, limit: u64) {
        if self.limit.is_none() {
            self.limit = Some(limit);
        }
    }

    pub fn set_tags(&mut self, tags: HashMap<String, Vec<String>>) {
        let mut t = HashMap::new();
        for item in tags {
            let key = item.0.into_bytes();
            // only index for key len 1
            if key.len() == 1 {
                let val = item
                    .1
                    .into_iter()
                    .map(|s| s.into_bytes())
                    // only index tag value length < 255
                    .filter(|s| s.len() < 255)
                    .collect::<Vec<_>>();
                if !key.is_empty() && !val.is_empty() {
                    t.insert(key, val.into());
                }
            }
        }
        self.tags = t;
    }

    pub fn match_id(ids: &SortList<[u8; 32]>, id: &[u8; 32]) -> bool {
        ids.is_empty() || ids.contains(id)
    }

    pub fn match_author(
        authors: &SortList<[u8; 32]>,
        pubkey: &[u8; 32],
        delegator: Option<&[u8; 32]>,
    ) -> bool {
        authors.is_empty()
            || Self::match_id(authors, pubkey)
            || delegator
                .map(|d| Self::match_id(authors, d))
                .unwrap_or_default()
    }

    pub fn match_kind(kinds: &SortList<u16>, kind: u16) -> bool {
        kinds.is_empty() || kinds.contains(&kind)
    }

    pub fn match_tag<V: AsRef<[u8]>, I: AsRef<[(V, V)]>>(
        tags: &HashMap<Vec<u8>, SortList<Vec<u8>>>,
        event_tags: I,
    ) -> bool {
        // empty tags
        if tags.is_empty() {
            return true;
        }

        // event has not tag
        if event_tags.as_ref().is_empty() {
            return false;
        }

        // all tag must match
        for tag in tags.iter() {
            if !Self::tag_contains(&event_tags, tag.0, tag.1) {
                return false;
            }
        }
        true
    }

    fn tag_contains<V: AsRef<[u8]>, I: AsRef<[(V, V)]>>(
        tags: I,
        name: &[u8],
        list: &SortList<Vec<u8>>,
    ) -> bool {
        let tags = tags.as_ref();
        if tags.is_empty() {
            return false;
        }
        for tag in tags {
            if tag.0.as_ref() == name {
                // Check for exact match first
                if list.contains2(tag.1.as_ref()) {
                    return true;
                }

                // Handle geohash prefix matching for g-tags
                if name == b"g" {
                    let tag_value = tag.1.as_ref();
                    for filter_value in list.iter() {
                        if Self::matches_geohash_prefix(tag_value, filter_value) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Check if a geohash tag value matches a prefix pattern
    fn matches_geohash_prefix(tag_value: &[u8], filter_value: &[u8]) -> bool {
        // Check if this is a prefix pattern (starts with ^)
        if !filter_value.starts_with(b"^") {
            return false;
        }

        let pattern = &filter_value[1..]; // Remove ^ prefix

        if let Some(colon_pos) = pattern.iter().position(|&b| b == b':') {
            // Format: ^prefix:max_length
            let prefix = &pattern[..colon_pos];
            if let Ok(max_len_str) = std::str::from_utf8(&pattern[colon_pos + 1..]) {
                if let Ok(max_len) = max_len_str.parse::<usize>() {
                    // Check prefix match and length constraint
                    return tag_value.starts_with(prefix) && tag_value.len() <= max_len;
                }
            }
        } else {
            // Format: ^prefix (no length constraint)
            return tag_value.starts_with(pattern);
        }

        false
    }

    pub fn r#match(&self, event: &EventIndex) -> bool {
        self.match_except_tag(event) && Self::match_tag(&self.tags, event.tags())
    }

    pub fn match_except_tag(&self, event: &EventIndex) -> bool {
        Self::match_id(&self.ids, event.id())
            && self.since.map_or(true, |t| event.created_at() >= t)
            && self.until.map_or(true, |t| event.created_at() <= t)
            && Self::match_kind(&self.kinds, event.kind())
            && Self::match_author(&self.authors, event.pubkey(), event.delegator())
    }

    pub fn match_archived(&self, event: &ArchivedEventIndex) -> bool {
        self.match_archived_except_tag(event) && Self::match_tag(&self.tags, event.tags())
    }

    pub fn match_archived_except_tag(&self, event: &ArchivedEventIndex) -> bool {
        Self::match_id(&self.ids, event.id())
            && self.since.map_or(true, |t| event.created_at() >= t)
            && self.until.map_or(true, |t| event.created_at() <= t)
            && Self::match_kind(&self.kinds, event.kind())
            && Self::match_author(&self.authors, event.pubkey(), event.delegator())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, str::FromStr};

    use super::Filter;
    use crate::{filter::SortList, ArchivedEventIndex, Event, EventIndex};
    use anyhow::Result;

    #[test]
    fn deser_filter() -> Result<()> {
        // empty
        let note = "{}";
        let filter: Filter = serde_json::from_str(note)?;
        assert!(filter.tags.is_empty());
        assert!(filter.ids.is_empty());

        // valid
        let note = r###"
        {
            "ids": ["abababababababababababababababababababababababababababababababab", "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd", "1212121212121212121212121212121212121212121212121212121212121212"],
            "authors": ["abababababababababababababababababababababababababababababababab", "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd", "1212121212121212121212121212121212121212121212121212121212121212"],
            "kinds": [2, 1],
            "until": 5,
            "since": 3,
            "limit": 6,
            "#d": ["ab", "cd", "12"],
            "#f": ["ab", "cd", "12", "ab"],
            "#b": [],
            "search": "abc",
            "invalid": ["ab", "cd", "12"],
            "_invalid": 123
          }
        "###;
        let mut filter: Filter = serde_json::from_str(note)?;
        let li = SortList::from(vec![[0x12; 32], [0xab; 32], [0xcd; 32]]);
        let tags: SortList<Vec<u8>> = ["ab", "cd", "12"]
            .iter()
            .map(|s| s.as_bytes().to_vec())
            .collect::<Vec<_>>()
            .into();
        assert_eq!(&filter.ids, &li);
        assert_eq!(&filter.authors, &li);
        assert_eq!(&filter.kinds, &SortList::from(vec![1, 2]));
        assert_eq!(filter.until, Some(5));
        assert_eq!(filter.since, Some(3));
        assert_eq!(filter.limit, Some(6));
        assert_eq!(filter.search, Some("abc".to_string()));

        // tag
        assert_eq!(
            &filter.tags.get(&"d".to_string().into_bytes()),
            &Some(&tags)
        );
        // dup
        assert_eq!(
            &filter.tags.get(&"f".to_string().into_bytes()),
            &Some(&tags)
        );
        assert!(filter
            .tags
            .get(&"invalid".to_string().into_bytes())
            .is_none());
        assert!(filter
            .tags
            .get(&"_invalid".to_string().into_bytes())
            .is_none());
        assert!(filter.tags.get(&"b".to_string().into_bytes()).is_none());
        // set tag
        filter.set_tags(HashMap::from([
            (
                "t".to_string(),
                vec![
                    "ab".to_string(),
                    "ab".to_string(),
                    "cd".to_string(),
                    "12".to_string(),
                ],
            ),
            (
                "g".to_string(),
                vec!["ab".to_string(), "cd".to_string(), "12".to_string()],
            ),
        ]));
        assert_eq!(
            &filter.tags.get(&"t".to_string().into_bytes()),
            &Some(&tags)
        );
        assert_eq!(
            &filter.tags.get(&"g".to_string().into_bytes()),
            &Some(&tags)
        );
        assert!(filter.tags.get(&"d".to_string().into_bytes()).is_none());

        // search
        let note = r###"
        {
            "keywords": ["abc", "def"]
          }
        "###;
        let filter: Filter = serde_json::from_str(note)?;
        assert_eq!(filter.search, Some("abc def".to_string()));

        let note = r###"
        {
            "keywords": ["abc", "def"],
            "search": "t"
          }
        "###;
        let filter: Filter = serde_json::from_str(note)?;
        assert_eq!(filter.search, Some("t".to_string()));

        // invalid
        let note = r###"
        {
            "#g": ["ab", "cd", 12]
          }
        "###;
        let filter: Result<Filter, _> = serde_json::from_str(note);
        assert!(filter.is_err());

        let note = r###"
        {
            "#e": ["ab"],
            "#p": ["ab"]
          }
        "###;
        let filter = Filter::from_str(note);
        assert!(filter.is_err());

        let note = r###"
        {
            "#e": ["0000000000000000000000000000000000000000000000000000000000000000"],
            "#p": ["0000000000000000000000000000000000000000000000000000000000000000"]
          }
        "###;
        let filter = Filter::from_str(note)?;
        assert!(filter
            .tags
            .get(&b"e".to_vec())
            .unwrap()
            .contains(&vec![0u8; 32]));
        let filter = Filter::from_str(note)?;
        assert!(filter
            .tags
            .get(&b"p".to_vec())
            .unwrap()
            .contains(&vec![0u8; 32]));
        Ok(())
    }

    fn check_match(
        s: &str,
        matched: bool,
        event: &Event,
        archived: &ArchivedEventIndex,
    ) -> Result<()> {
        let filter: Filter = serde_json::from_str(s)?;
        if matched {
            assert!(filter.r#match(event.index()));
            assert!(filter.match_archived(archived));
        } else {
            assert!(!filter.r#match(event.index()));
            assert!(!filter.match_archived(archived));
        }
        Ok(())
    }

    #[test]
    fn match_event() -> Result<()> {
        let note = r#"
        {
            "content": "Good morning everyone 😃",
            "created_at": 1680690006,
            "id": "332747c0fab8a1a92def4b0937e177be6df4382ce6dd7724f86dc4710b7d4d7d",
            "kind": 1,
            "pubkey": "7abf57d516b1ff7308ca3bd5650ea6a4674d469c7c5057b1d005fb13d218bfef",
            "sig": "ef4ff4f69ac387239eb1401fb07d7a44a5d5d57127e0dc3466a0403cf7d5486b668608ebfcbe9ff1f8d3b5d710545999fe08ee767284ec0b474e4cf92537678f",
            "tags": [["t", "nostr"], ["t", "db"], ["subject", "db"]]
          }
        "#;
        let event: Event = serde_json::from_str(note)?;
        let bytes = event.index().to_bytes()?;
        let archived = EventIndex::from_zeroes(&bytes)?;

        check_match(
            r###"
        {
        }
        "###,
            true,
            &event,
            archived,
        )?;

        check_match(
            r###"
        {
            "ids": ["332747c0fab8a1a92def4b0937e177be6df4382ce6dd7724f86dc4710b7d4d7d", "0000000000000000000000000000000000000000000000000000000000000000"],
            "authors": ["7abf57d516b1ff7308ca3bd5650ea6a4674d469c7c5057b1d005fb13d218bfef", "0000000000000000000000000000000000000000000000000000000000000000"],
            "kind": [1, 2],
            "#t": ["nostr", "other"],
            "#subject": ["db", "other"],
            "since": 1680690000,
            "util": 2680690000
        }
        "###,
            true,
            &event,
            archived,
        )?;

        check_match(
            r###"
        {
            "#t": ["other"]
        }
        "###,
            false,
            &event,
            archived,
        )?;

        check_match(
            r###"
        {
            "#t": ["nostr"],
            "#r": ["nostr"]
        }
        "###,
            false,
            &event,
            archived,
        )?;

        check_match(
            r###"
        {
            "ids": ["332747c0fab8a1a92def4b0937e177be6df4382ce6dd7724f86dc4710b7d4d7d"]
        }
        "###,
            true,
            &event,
            archived,
        )?;

        check_match(
            r###"
        {
            "ids": ["abababababababababababababababababababababababababababababababab"]
        }
        "###,
            false,
            &event,
            archived,
        )?;

        Ok(())
    }

    #[test]
    fn tag_contains() -> Result<()> {
        let note = r#"
        {
            "content": "Good morning everyone 😃",
            "created_at": 1680690006,
            "id": "332747c0fab8a1a92def4b0937e177be6df4382ce6dd7724f86dc4710b7d4d7d",
            "kind": 1,
            "pubkey": "7abf57d516b1ff7308ca3bd5650ea6a4674d469c7c5057b1d005fb13d218bfef",
            "sig": "ef4ff4f69ac387239eb1401fb07d7a44a5d5d57127e0dc3466a0403cf7d5486b668608ebfcbe9ff1f8d3b5d710545999fe08ee767284ec0b474e4cf92537678f",
            "tags": [["t", "nostr"], ["t", "db"], ["r", "db"]]
          }
        "#;
        let event: Event = serde_json::from_str(note)?;
        assert!(Filter::tag_contains(
            event.index().tags(),
            &"t".to_string().into_bytes(),
            &vec!["nostr".to_string().into_bytes()].into()
        ));
        assert!(Filter::tag_contains(
            event.index().tags(),
            &"t".to_string().into_bytes(),
            &vec![
                "nostr".to_string().into_bytes(),
                "other".to_string().into_bytes()
            ]
            .into()
        ));

        assert!(!Filter::tag_contains(
            event.index().tags(),
            &"t".to_string().into_bytes(),
            &vec![
                "nostr1".to_string().into_bytes(),
                "other".to_string().into_bytes()
            ]
            .into()
        ));
        Ok(())
    }

    #[test]
    fn test_geohash_prefix_parsing() -> Result<()> {
        // Test basic #g^ parsing with array format
        let note = r##"{"#g^": ["abc"]}"##;
        let filter: Filter = serde_json::from_str(note)?;
        assert!(filter.tags.contains_key(&b"g".to_vec()));
        let g_values = filter.tags.get(&b"g".to_vec()).unwrap();
        assert!(g_values.contains(&b"^abc".to_vec()));

        // Test #g^N parsing with length constraint
        let note = r##"{"#g^6": ["def"]}"##;
        let filter: Filter = serde_json::from_str(note)?;
        let g_values = filter.tags.get(&b"g".to_vec()).unwrap();
        assert!(g_values.contains(&b"^def:6".to_vec()));

        // Test multiple geohash prefixes
        let note = r##"{"#g^": ["abc", "xyz"]}"##;
        let filter: Filter = serde_json::from_str(note)?;
        let g_values = filter.tags.get(&b"g".to_vec()).unwrap();
        assert!(g_values.contains(&b"^abc".to_vec()));
        assert!(g_values.contains(&b"^xyz".to_vec()));

        Ok(())
    }

    #[test]
    fn test_geohash_prefix_matching() -> Result<()> {
        // Create a mock event with geohash tags
        let event_tags = vec![
            (b"g".to_vec(), b"abcde".to_vec()),
            (b"g".to_vec(), b"xyz123".to_vec()),
            (b"t".to_vec(), b"nostr".to_vec()),
        ];

        // Test prefix matching without length constraint
        assert!(Filter::matches_geohash_prefix(b"abcde", b"^abc"));
        assert!(Filter::matches_geohash_prefix(b"abcdefg", b"^abc"));
        assert!(!Filter::matches_geohash_prefix(b"xyzde", b"^abc"));

        // Test prefix matching with length constraint
        assert!(Filter::matches_geohash_prefix(b"abc", b"^ab:3"));
        assert!(Filter::matches_geohash_prefix(b"ab", b"^ab:3"));
        assert!(!Filter::matches_geohash_prefix(b"abcd", b"^ab:3")); // too long
        assert!(!Filter::matches_geohash_prefix(b"xyz", b"^ab:3")); // wrong prefix

        // Test tag_contains with geohash prefixes
        let filter_values = vec![b"^abc".to_vec(), b"^xyz:6".to_vec()].into();
        assert!(Filter::tag_contains(&event_tags, b"g", &filter_values));

        let filter_values_no_match = vec![b"^def".to_vec()].into();
        assert!(!Filter::tag_contains(&event_tags, b"g", &filter_values_no_match));

        Ok(())
    }
}
