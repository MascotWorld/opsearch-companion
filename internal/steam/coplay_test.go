package steam

import (
	"encoding/binary"
	"testing"
	"time"
)

func sampleBinary(owner, peer uint64, ownerAt, peerAt int32) []byte {
	var b []byte
	str := func(s string) { b = append(b, []byte(s)...); b = append(b, 0) }
	entry := func(idx string, id uint64, at int32) {
		b = append(b, 0)
		str(idx)
		b = append(b, 7)
		str("SteamID")
		var q [8]byte
		binary.LittleEndian.PutUint64(q[:], id)
		b = append(b, q[:]...)
		b = append(b, 7)
		str("gameid")
		binary.LittleEndian.PutUint64(q[:], 730)
		b = append(b, q[:]...)
		b = append(b, 2)
		str("Playtime")
		var x [4]byte
		binary.LittleEndian.PutUint32(x[:], uint32(at))
		b = append(b, x[:]...)
		b = append(b, 8)
	}
	b = append(b, 0)
	str("coplay")
	entry("0", owner, ownerAt)
	entry("1", peer, peerAt)
	entry("2", 76561198000000002, ownerAt-121)
	b = append(b, 8, 8)
	return b
}
func TestParseCoplayOwnerAnchoredRecentCohort(t *testing.T) {
	now := time.Unix(1793094741, 0)
	b := sampleBinary(76561198000000000, 76561198000000001, 1793094741, 1793094714)
	r, e := ParseCoplay(b, "76561198000000000", now)
	if e != nil {
		t.Fatal(e)
	}
	if r.OwnerPlaytime != 1793094741 || len(r.Players) != 1 || r.Players[0].SteamID64 != "76561198000000001" || r.Players[0].Playtime != 1793094714 {
		t.Fatalf("unexpected roster: %+v", r)
	}
}
func TestParseCoplayRetainsOldOwnerAnchoredCohort(t *testing.T) {
	b := sampleBinary(76561198000000000, 76561198000000001, 1700000000, 1699999999)
	r, e := ParseCoplay(b, "76561198000000000", time.Unix(1793094741, 0))
	if e != nil {
		t.Fatal(e)
	}
	if r.OwnerPlaytime != 1700000000 || len(r.Players) != 1 || r.Players[0].Playtime != 1699999999 {
		t.Fatalf("historical cohort was not retained: %+v", r)
	}
}
func TestParseCoplayRejectsTimestampBeyondBackendFutureTolerance(t *testing.T) {
	b := sampleBinary(76561198000000000, 76561198000000001, 1793094802, 1793094802)
	if _, e := ParseCoplay(b, "76561198000000000", time.Unix(1793094741, 0)); e == nil {
		t.Fatal("expected owner timestamp over 60 seconds in the future to be rejected")
	}
}
