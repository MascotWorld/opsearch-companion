package client

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestPairRequestAndResponse(t *testing.T) {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/cs2/companion/pair" {
			t.Fatalf("path %s", r.URL.Path)
		}
		var p PairRequest
		if e := json.NewDecoder(r.Body).Decode(&p); e != nil {
			t.Fatal(e)
		}
		if p.Code != "ABCDEFGHIJKL" || p.SteamID64 != "76561198000000000" {
			t.Fatalf("request %+v", p)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"device_id":"dev","token":"secret","steamid64":"76561198000000000"}`))
	}))
	defer s.Close()
	r, e := New(s.URL).Pair(context.Background(), PairRequest{Code: "ABCDEFGHIJKL", SteamID64: "76561198000000000", DeviceName: "pc", ClientVersion: Version})
	if e != nil {
		t.Fatal(e)
	}
	if r.Token != "secret" {
		t.Fatal("token missing")
	}
}
func TestSnapshotAuthorizationAndPayload(t *testing.T) {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer private" {
			t.Fatal("missing scoped authorization")
		}
		var p Snapshot
		if e := json.NewDecoder(r.Body).Decode(&p); e != nil {
			t.Fatal(e)
		}
		if p.OwnerPlaytime != 1793094741 || len(p.Players) != 1 || p.Players[0].AppID != 730 || p.Players[0].Playtime != 1793094714 {
			t.Fatalf("payload %+v", p)
		}
		w.WriteHeader(http.StatusAccepted)
	}))
	defer s.Close()
	e := New(s.URL).Snapshot(context.Background(), "private", Snapshot{SourceSteamID64: "76561198000000000", LocalSteamID64: "76561198000000000", ObservedAt: time.Now().UTC(), OwnerPlaytime: 1793094741, Players: []Player{{SteamID64: "76561198000000001", Playtime: 1793094714, AppID: 730}}, ClientVersion: Version})
	if e != nil {
		t.Fatal(e)
	}
}
func TestPairRedirectDoesNotForwardCredentials(t *testing.T) {
	var leaked bool
	dest := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { leaked = true }))
	defer dest.Close()
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { http.Redirect(w, r, dest.URL, http.StatusFound) }))
	defer s.Close()
	if _, e := New(s.URL).Pair(context.Background(), PairRequest{Code: "ABCDEFGHIJKL", SteamID64: "76561198000000000"}); e == nil {
		t.Fatal("expected redirect failure")
	}
	if leaked {
		t.Fatal("pair body followed redirect")
	}
}

func TestValidateServerRequiresOriginAndSecureTransport(t *testing.T) {
	for _, good := range []string{"https://opsearch.pro", "https://example.test/", "http://127.0.0.1:4000", "http://[::1]:4000"} {
		if e := ValidateServer(good); e != nil {
			t.Errorf("%q rejected: %v", good, e)
		}
	}
	for _, bad := range []string{"http://example.test", "https://u:p@example.test", "https://example.test/path", "https://example.test/?token=x", "https://example.test/#x"} {
		if e := ValidateServer(bad); e == nil {
			t.Errorf("%q accepted", bad)
		}
	}
}
