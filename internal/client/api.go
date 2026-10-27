package client

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"
)

const Version = "opsearch-companion/0.1.0"
const DefaultServer = "https://opsearch.pro"

type PairRequest struct {
	Code          string `json:"code"`
	SteamID64     string `json:"steamid64"`
	DeviceName    string `json:"device_name"`
	ClientVersion string `json:"client_version"`
}
type PairResponse struct {
	DeviceID  string `json:"device_id"`
	Token     string `json:"token"`
	SteamID64 string `json:"steamid64"`
}
type Player struct {
	SteamID64 string `json:"steamid64"`
	Playtime  int64  `json:"playtime"`
	AppID     uint32 `json:"app_id"`
}
type Snapshot struct {
	SourceSteamID64 string    `json:"source_steamid64"`
	LocalSteamID64  string    `json:"local_steamid64"`
	ObservedAt      time.Time `json:"observed_at"`
	OwnerPlaytime   int64     `json:"owner_playtime"`
	Players         []Player  `json:"players"`
	ClientVersion   string    `json:"client_version"`
}
type API struct {
	BaseURL string
	HTTP    *http.Client
}

func New(base string) *API {
	if base == "" {
		base = DefaultServer
	}
	return &API{BaseURL: strings.TrimRight(base, "/"), HTTP: &http.Client{Timeout: 15 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}
}

func ValidateServer(raw string) error {
	u, e := url.Parse(raw)
	if e != nil || u.Host == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" || u.Path != "" && u.Path != "/" {
		return errors.New("invalid server origin")
	}
	if u.Scheme == "https" {
		return nil
	}
	if u.Scheme == "http" && (u.Hostname() == "localhost" || u.Hostname() == "127.0.0.1" || u.Hostname() == "::1") {
		return nil
	}
	return errors.New("server must use HTTPS; plain HTTP is allowed only for localhost")
}
func (a *API) post(ctx context.Context, path, token string, value any) ([]byte, error) {
	body, e := json.Marshal(value)
	if e != nil {
		return nil, e
	}
	req, e := http.NewRequestWithContext(ctx, http.MethodPost, a.BaseURL+path, bytes.NewReader(body))
	if e != nil {
		return nil, e
	}
	req.Header.Set("Content-Type", "application/json")
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	resp, e := a.HTTP.Do(req)
	if e != nil {
		return nil, e
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("server returned HTTP %s", resp.Status)
	}
	var out bytes.Buffer
	if _, e = out.ReadFrom(resp.Body); e != nil {
		return nil, e
	}
	return out.Bytes(), nil
}
func (a *API) Pair(ctx context.Context, p PairRequest) (PairResponse, error) {
	if e := ValidateServer(a.BaseURL); e != nil {
		return PairResponse{}, e
	}
	b, e := a.post(ctx, "/api/cs2/companion/pair", "", p)
	if e != nil {
		return PairResponse{}, e
	}
	var r PairResponse
	if e = json.Unmarshal(b, &r); e != nil {
		return r, e
	}
	if r.Token == "" || r.DeviceID == "" || r.SteamID64 != p.SteamID64 {
		return r, errors.New("server returned an invalid device pairing response")
	}
	return r, nil
}
func (a *API) Snapshot(ctx context.Context, token string, s Snapshot) error {
	if e := ValidateServer(a.BaseURL); e != nil {
		return e
	}
	_, e := a.post(ctx, "/api/cs2/companion/snapshots", token, s)
	return e
}
