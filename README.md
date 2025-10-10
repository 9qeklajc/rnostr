# rnostr-geo

A fork of [rnostr](https://github.com/rnostr/rnostr) with **geohash prefix search** functionality added.

## What's Added

This fork adds **geohash prefix search** capabilities to the rnostr relay, enabling efficient location-based queries using geohash tags.

### ✨ New Features

- **Geohash Prefix Queries**: Search for events by geohash prefix patterns
- **Length-Constrained Search**: Limit search by maximum geohash length
- **Optimized Database Queries**: Efficient LMDB range queries for prefix matching
- **Proper Validation**: Validates geohash characters and format

### 🔍 Query Syntax

```json
{"#g^": ["u"]}           // Find all geohashes starting with "u"
{"#g^": ["u1", "9q"]}    // Find geohashes starting with "u1" OR "9q"
{"#g^6": ["u12"]}        // Find geohashes starting with "u12", max length 6
```

### 📍 Example Use Cases

```bash
# Find all events in San Francisco area (geohash prefix "u1")
nak req -t g^=u1 ws://localhost:8080

# Find events in broader SF Bay Area with length limit
nak req -t g^4=u1 ws://localhost:8080
```

## Quick Start

### Prerequisites

- Rust toolchain
- [nak](https://github.com/fiatjaf/nak) tool for testing
- [just](https://github.com/casey/just) command runner (optional)

## Implementation Details

### Files Modified

- `db/src/filter.rs`: Filter parsing and validation
- `db/src/db.rs`: Database query optimization

## Testing

```bash
# Run geohash prefix search test
just test-prefix

# Full automated test workflow
just test-full
```

## License

MIT OR Apache-2.0 (same as original rnostr project)

## Geohash Resources
- [Geohash Wikipedia](https://en.wikipedia.org/wiki/Geohash)
- [Geohash Algorithm](https://www.movable-type.co.uk/scripts/geohash.html)
- [Geohash Explorer](https://geohash.softeng.co/)
