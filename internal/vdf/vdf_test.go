package vdf

import (
	"encoding/binary"
	"testing"
)

func TestDecodeNestedEscapedCaseInsensitive(t *testing.T) {
	n, err := Decode([]byte(`"Root" { "Owner" "abc\\\"d" "Items" { "730" { "SteamID" "76561198000000000" } } }`))
	if err != nil {
		t.Fatal(err)
	}
	r, ok := n.Get("root")
	if !ok || r.Object == nil {
		t.Fatal("root object missing")
	}
	owner, _ := r.Object.Get("owner")
	if owner.Text != `abc\"d` {
		t.Fatalf("got %q", owner.Text)
	}
	items, _ := r.Object.Get("ITEMS")
	id, _ := items.Object.Get("730")
	sid, _ := id.Object.Get("SteamID")
	if sid.Text != "76561198000000000" {
		t.Fatalf("got %q", sid.Text)
	}
}
func TestDecodeUTF16LE(t *testing.T) {
	s := `"a" "b"`
	u := []uint16{}
	for _, r := range s {
		u = append(u, uint16(r))
	}
	b := []byte{0xff, 0xfe}
	for _, x := range u {
		z := make([]byte, 2)
		binary.LittleEndian.PutUint16(z, x)
		b = append(b, z...)
	}
	n, e := Decode(b)
	if e != nil {
		t.Fatal(e)
	}
	v, ok := n.Get("a")
	if !ok || v.Text != "b" {
		t.Fatalf("%+v", v)
	}
}
func TestDecodeMalformed(t *testing.T) {
	for _, s := range []string{`"a" {`, `"a"`, `}`, `"a" }`} {
		if _, e := Decode([]byte(s)); e == nil {
			t.Errorf("expected error for %q", s)
		}
	}
}

func TestDecodeBinarySteamKeyValues(t *testing.T) {
	var b []byte
	appendString := func(s string) { b = append(b, []byte(s)...); b = append(b, 0) }
	b = append(b, 0)
	appendString("coplay")
	b = append(b, 0)
	appendString("0")
	b = append(b, 7)
	appendString("SteamID")
	var q [8]byte
	binary.LittleEndian.PutUint64(q[:], 76561198000000000)
	b = append(b, q[:]...)
	b = append(b, 7)
	appendString("gameid")
	binary.LittleEndian.PutUint64(q[:], 730)
	b = append(b, q[:]...)
	b = append(b, 2)
	appendString("Playtime")
	var d [4]byte
	binary.LittleEndian.PutUint32(d[:], 1793094741)
	b = append(b, d[:]...)
	b = append(b, 8, 8, 8)
	n, e := Decode(b)
	if e != nil {
		t.Fatal(e)
	}
	root, ok := n.Get("coplay")
	if !ok || root.Object == nil {
		t.Fatal("binary root missing")
	}
	entry, ok := root.Object.Get("0")
	if !ok || entry.Object == nil {
		t.Fatal("binary entry missing")
	}
	id, _ := entry.Object.Get("SteamID")
	if id.Text != "76561198000000000" {
		t.Fatalf("SteamID=%q", id.Text)
	}
	game, _ := entry.Object.Get("gameid")
	if game.Text != "730" {
		t.Fatalf("gameid=%q", game.Text)
	}
	when, _ := entry.Object.Get("Playtime")
	if when.Text != "1793094741" {
		t.Fatalf("Playtime=%q", when.Text)
	}
}

func TestDecodeBinaryTruncatedAndMissingEnd(t *testing.T) {
	for _, b := range [][]byte{{0, 'x', 0, 0, '0', 0, 7, 'i', 0, 1}, {0, 'x', 0, 0, '0', 0, 7, 'i', 0, 1, 2, 3, 8}} {
		if _, e := Decode(b); e == nil {
			t.Errorf("expected binary parse error for %v", b)
		}
	}
}
