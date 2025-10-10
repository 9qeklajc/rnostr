#!/bin/bash

# Simple test script for geohash prefix searches
# Assumes relay is already running on ws://localhost:8080

RELAY_URL="ws://localhost:8080"
TEST_NSEC="nsec1ufnus6pju578ste3v90xd5m2decpuzpql2295m3sknqcjzyys9ls0qlc85"

echo "Geohash Prefix Search Test"
echo "=========================="
echo ""
echo "Adding test events to database..."

# Add events with various geohash tags
echo "Publishing events with geohash tags..."

# San Francisco area geohashes
nak event --sec $TEST_NSEC --content "Golden Gate Bridge" --tag g="u1" $RELAY_URL
nak event --sec $TEST_NSEC --content "Fisherman's Wharf" --tag g="u12" $RELAY_URL
nak event --sec $TEST_NSEC --content "Union Square" --tag g="u123" $RELAY_URL
nak event --sec $TEST_NSEC --content "Mission District" --tag g="u1234" $RELAY_URL
nak event --sec $TEST_NSEC --content "Castro District" --tag g="u12345" $RELAY_URL

# New York area geohashes
nak event --sec $TEST_NSEC --content "Times Square" --tag g="dr5r" $RELAY_URL
nak event --sec $TEST_NSEC --content "Central Park" --tag g="dr5r7" $RELAY_URL
nak event --sec $TEST_NSEC --content "Brooklyn Bridge" --tag g="dr5r78" $RELAY_URL

# Los Angeles area geohashes
nak event --sec $TEST_NSEC --content "Hollywood Sign" --tag g="9q5" $RELAY_URL
nak event --sec $TEST_NSEC --content "Santa Monica Pier" --tag g="9q58" $RELAY_URL
nak event --sec $TEST_NSEC --content "Venice Beach" --tag g="9q589" $RELAY_URL

echo ""
echo "Waiting for indexing..."
sleep 3

echo ""
echo "Testing prefix searches..."
echo "========================="

echo ""
echo "1. Basic prefix search - all 'u' locations (San Francisco area):"
echo "Command: nak req -t g^=u $RELAY_URL"
nak req -t g^=u $RELAY_URL
echo ""

echo "2. More specific prefix - 'u1' locations:"
echo "Command: nak req -t g^=u1 $RELAY_URL"
nak req -t g^=u1 $RELAY_URL
echo ""

echo "3. Even more specific - 'u12' locations:"
echo "Command: nak req -t g^=u12 $RELAY_URL"
nak req -t g^=u12 $RELAY_URL
echo ""

echo "4. New York area - 'dr5r' prefix:"
echo "Command: nak req -t g^=dr5r $RELAY_URL"
nak req -t g^=dr5r $RELAY_URL
echo ""

echo "5. Los Angeles area - '9q5' prefix:"
echo "Command: nak req -t g^=9q5 $RELAY_URL"
nak req -t g^=9q5 $RELAY_URL
echo ""

echo "6. Length-constrained search - 'u' with max length 3:"
echo "Command: nak req -t g^3=u $RELAY_URL"
nak req -t g^3=u $RELAY_URL
echo ""

echo "7. Length-constrained search - 'u1' with max length 4:"
echo "Command: nak req -t g^4=u1 $RELAY_URL"
nak req -t g^4=u1 $RELAY_URL
echo ""

echo "8. Non-matching prefix search (should return nothing):"
echo "Command: nak req -t g^=xyz $RELAY_URL"
nak req -t g^=xyz $RELAY_URL
echo ""

echo "9. Regular tag search for comparison - exact match 'u123':"
echo "Command: nak req -t g=u123 $RELAY_URL"
nak req -t g=u123 $RELAY_URL
echo ""

echo "Test completed!"
echo ""
echo "Expected results:"
echo "- Query 1 (g^=u): Should return 5 events (all u* geohashes)"
echo "- Query 2 (g^=u1): Should return 5 events (all u1* geohashes)"
echo "- Query 3 (g^=u12): Should return 4 events (u12, u123, u1234, u12345)"
echo "- Query 4 (g^=dr5r): Should return 3 events (all dr5r* geohashes)"
echo "- Query 5 (g^=9q5): Should return 3 events (all 9q5* geohashes)"
echo "- Query 6 (g^3=u): Should return 2 events (u1, u12 only - length ≤ 3)"
echo "- Query 7 (g^4=u1): Should return 3 events (u1, u12, u123 only - length ≤ 4)"
echo "- Query 8 (g^=xyz): Should return 0 events"
echo "- Query 9 (g=u123): Should return 1 event (exact match only)"