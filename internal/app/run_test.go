package app

import (
	"context"
	"encoding/binary"
	"errors"
	"os"
	"path/filepath"
	"strconv"
	"testing"
	"time"

	"github.com/MascotWorld/opsearch-companion/internal/client"
	"github.com/MascotWorld/opsearch-companion/internal/steam"
)

func makeCoplay(owner, peer uint64, at int32) []byte {
	var b []byte
	z := func(s string) { b = append(b, []byte(s)...); b = append(b, 0) }
	entry := func(i string, id uint64, ts int32) {
		b = append(b, 0)
		z(i)
		b = append(b, 7)
		z("SteamID")
		var q [8]byte
		binary.LittleEndian.PutUint64(q[:], id)
		b = append(b, q[:]...)
		b = append(b, 7)
		z("gameid")
		binary.LittleEndian.PutUint64(q[:], 730)
		b = append(b, q[:]...)
		b = append(b, 2)
		z("Playtime")
		var x [4]byte
		binary.LittleEndian.PutUint32(x[:], uint32(ts))
		b = append(b, x[:]...)
		b = append(b, 8)
	}
	b = append(b, 0)
	z("coplay")
	entry("0", owner, at)
	entry("1", peer, at)
	b = append(b, 8, 8)
	return b
}
func TestRunSwitchesCredentialsWithActiveSteamAccount(t *testing.T) {
	dir := t.TempDir()
	const a = "76561198000000000"
	const b = "76561198000000002"
	roots := map[string]string{}
	for _, x := range []struct{ id, peer string }{{a, "76561198000000001"}, {b, "76561198000000003"}} {
		root := filepath.Join(dir, x.id)
		roots[x.id] = root
		cfgdir := filepath.Join(root, "config")
		if e := os.MkdirAll(cfgdir, 0700); e != nil {
			t.Fatal(e)
		}
		var owner, peer uint64
		owner, _ = strconv.ParseUint(x.id, 10, 64)
		peer, _ = strconv.ParseUint(x.peer, 10, 64)
		if e := os.WriteFile(filepath.Join(cfgdir, "coplay_"+x.id+".vdf"), makeCoplay(owner, peer, 1793094741), 0600); e != nil {
			t.Fatal(e)
		}
	}
	config := Config{Devices: map[string]Device{a: {DeviceID: "a", Token: "token-a", Server: "https://prod-a.example"}, b: {DeviceID: "b", Token: "token-b", Server: "https://test-b.example"}}}
	cfgpath := filepath.Join(dir, "config.json")
	if e := SaveConfig(cfgpath, config); e != nil {
		t.Fatal(e)
	}
	sequence := []string{a, b, "", b}
	reads := 0
	active := func() (steam.Account, error) {
		if reads >= len(sequence) {
			return steam.Account{}, errors.New("unexpected extra read")
		}
		id := sequence[reads]
		reads++
		if id == "" {
			return steam.Account{}, errors.New("signed out")
		}
		return steam.Account{SteamID64: id, SteamRoot: roots[id]}, nil
	}
	type sent struct{ owner, token, server string }
	var got []sent
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	upload := func(_ context.Context, server, token string, s client.Snapshot) error {
		got = append(got, sent{s.LocalSteamID64, token, server})
		if len(got) == 3 {
			cancel()
		}
		return nil
	}
	e := runWith(ctx, cfgpath, false, false, active, upload, time.Millisecond, func() time.Time { return time.Unix(1793094741, 0) })
	if e != nil {
		t.Fatal(e)
	}
	want := []sent{{a, "token-a", "https://prod-a.example"}, {b, "token-b", "https://test-b.example"}, {b, "token-b", "https://test-b.example"}}
	if len(got) != len(want) {
		t.Fatalf("sent %d requests: %+v", len(got), got)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("request %d = %+v, want %+v", i, got[i], want[i])
		}
	}
}

func TestFingerprintCoplayFilesDetectsMultipleAccountsAndAtomicReplacement(t *testing.T) {
	dir := t.TempDir()
	a := filepath.Join(dir, "coplay_76561198000000000.vdf")
	b := filepath.Join(dir, "coplay_76561198000000001.vdf")
	if e := os.WriteFile(a, []byte("first"), 0600); e != nil {
		t.Fatal(e)
	}
	before, e := fingerprintCoplayFiles(dir)
	if e != nil || len(before) != 1 {
		t.Fatalf("initial scan %d %v", len(before), e)
	}
	if e = os.WriteFile(b, []byte("second"), 0600); e != nil {
		t.Fatal(e)
	}
	added, e := fingerprintCoplayFiles(dir)
	if e != nil || sameFingerprints(before, added) {
		t.Fatal("new account file change was not detected")
	}
	tmp := a + ".tmp"
	if e = os.WriteFile(tmp, []byte("replacement"), 0600); e != nil {
		t.Fatal(e)
	}
	if e = os.Rename(tmp, a); e != nil {
		t.Fatal(e)
	}
	replaced, e := fingerprintCoplayFiles(dir)
	if e != nil || sameFingerprints(added, replaced) {
		t.Fatal("atomic replacement was not detected")
	}
	if e = os.Remove(b); e != nil {
		t.Fatal(e)
	}
	deleted, e := fingerprintCoplayFiles(dir)
	if e != nil || sameFingerprints(replaced, deleted) {
		t.Fatal("removed account file was not detected")
	}
}
