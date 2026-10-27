package steam

import (
	"errors"
	"fmt"
	"sort"
	"strconv"
	"time"

	"github.com/MascotWorld/opsearch-companion/internal/vdf"
)

const CS2AppID = 730
const cohortWindow = 2 * time.Minute
const maxUnixTime = int64(2_147_483_647)

type Player struct {
	SteamID64 string `json:"steamid64"`
	Playtime  int64  `json:"playtime"`
	AppID     uint32 `json:"app_id"`
}
type Roster struct {
	OwnerSteamID64 string
	OwnerPlaytime  int64
	Players        []Player
}

// ParseCoplay reads config/coplay_<owner>.vdf. Steam currently writes this as
// binary KV1: coplay/<index>/{SteamID:uint64,gameid:uint64,Playtime:int32}.
func ParseCoplay(data []byte, owner string, now time.Time) (Roster, error) {
	if !validSteamID(owner) {
		return Roster{}, errors.New("invalid owner SteamID64")
	}
	n, err := vdf.Decode(data)
	if err != nil {
		return Roster{}, fmt.Errorf("parse Coplay VDF: %w", err)
	}
	r, ok := n.Get("coplay")
	if !ok || r.Object == nil {
		return Roster{}, errors.New("Coplay VDF root object is missing")
	}
	type row struct {
		id  string
		app uint64
		at  int64
	}
	rows := make([]row, 0, len(r.Object.Pairs))
	var ownerTime int64
	for _, p := range r.Object.Pairs {
		if p.Value.Object == nil {
			continue
		}
		idv, iok := p.Value.Object.Get("SteamID")
		appv, aok := p.Value.Object.Get("gameid")
		timev, tok := p.Value.Object.Get("Playtime")
		if !iok || !aok || !tok {
			continue
		}
		id := idv.Text
		if !validSteamID(id) {
			continue
		}
		app, e := strconv.ParseUint(appv.Text, 10, 64)
		if e != nil {
			continue
		}
		at, e := strconv.ParseInt(timev.Text, 10, 64)
		if e != nil || at <= 0 || at > maxUnixTime {
			continue
		}
		rows = append(rows, row{id, app, at})
		if id == owner && app == CS2AppID && at > ownerTime {
			ownerTime = at
		}
	}
	if ownerTime == 0 {
		return Roster{}, errors.New("owner's CS2 entry is missing from Coplay VDF")
	}
	if time.Unix(ownerTime, 0).After(now.Add(time.Minute)) {
		return Roster{}, errors.New("owner CS2 timestamp is more than one minute in the future")
	}
	players := make([]Player, 0, 64)
	seen := map[string]bool{}
	for _, p := range rows {
		if p.app != CS2AppID || p.id == owner || seen[p.id] || time.Unix(p.at, 0).After(now.Add(time.Minute)) || time.Duration(abs64(p.at-ownerTime))*time.Second > cohortWindow {
			continue
		}
		seen[p.id] = true
		players = append(players, Player{p.id, p.at, CS2AppID})
	}
	if len(players) > 64 {
		sort.Slice(players, func(i, j int) bool {
			di, dj := abs64(players[i].Playtime-ownerTime), abs64(players[j].Playtime-ownerTime)
			if di == dj {
				return players[i].SteamID64 < players[j].SteamID64
			}
			return di < dj
		})
		players = players[:64]
	}
	return Roster{owner, ownerTime, players}, nil
}

func validSteamID(s string) bool {
	if len(s) != 17 {
		return false
	}
	n, e := strconv.ParseUint(s, 10, 64)
	return e == nil && n >= 76561197960265728 && n <= 76561202255233023
}
func abs64(n int64) int64 {
	if n < 0 {
		return -n
	}
	return n
}
