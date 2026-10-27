package app

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/MascotWorld/opsearch-companion/internal/client"
	"github.com/MascotWorld/opsearch-companion/internal/steam"
)

const pollEvery = 5 * time.Second

func Inspect(file, owner string, now time.Time) (steam.Roster, error) {
	data, err := os.ReadFile(file)
	if err != nil {
		return steam.Roster{}, err
	}
	return steam.ParseCoplay(data, owner, now)
}

func Run(ctx context.Context, configPath string, once bool, diagnose bool) error {
	return runWith(ctx, configPath, once, diagnose, steam.ActiveAccount, nil, pollEvery, time.Now)
}

type activeReader func() (steam.Account, error)
type uploader func(context.Context, string, string, client.Snapshot) error

func runWith(ctx context.Context, configPath string, once, diagnose bool, activeReader activeReader, upload uploader, interval time.Duration, now func() time.Time) error {
	var activeOwner string
	var lastHash string
	var lastPost time.Time
	var watchedRoot string
	var fileFingerprints map[string]string
	for {
		if err := ctx.Err(); err != nil {
			return nil
		}
		account, err := activeReader()
		if err != nil {
			if once {
				return err
			}
			if activeOwner != "" {
				activeOwner = ""
				lastHash = ""
				lastPost = time.Time{}
			}
			fmt.Fprintln(os.Stderr, "Waiting for a signed-in Steam account:", err)
			if !pause(ctx, interval) {
				return nil
			}
			continue
		}
		if account.SteamID64 != activeOwner {
			activeOwner = account.SteamID64
			lastHash = ""
			lastPost = time.Time{}
			fmt.Printf("Active Steam account: %s\n", account.SteamID64)
		}
		coplayDir := filepath.Join(account.SteamRoot, "config")
		if account.SteamRoot != watchedRoot {
			watchedRoot = account.SteamRoot
			fileFingerprints = nil
		}
		currentFiles, scanErr := fingerprintCoplayFiles(coplayDir)
		if scanErr != nil {
			fmt.Fprintln(os.Stderr, "Could not scan Steam Coplay cache:", scanErr)
		} else {
			if fileFingerprints != nil && !sameFingerprints(fileFingerprints, currentFiles) {
				fmt.Println("Steam Coplay cache changed; checking the active account roster.")
			}
			fileFingerprints = currentFiles
		}
		cfg, err := LoadConfig(configPath)
		if err != nil && !errors.Is(err, os.ErrNotExist) {
			return fmt.Errorf("read config: %w", err)
		}
		device, paired := cfg.Devices[account.SteamID64]
		if !paired || device.Token == "" || device.Server == "" {
			if once || diagnose {
				roster, _, e := readCurrent(coplayDir, account.SteamID64, now())
				if e != nil {
					return e
				}
				fmt.Printf("Coplay file ready: %d owner-anchored CS2 peers; owner playtime %s UTC. No upload performed.\n", len(roster.Players), time.Unix(roster.OwnerPlaytime, 0).UTC().Format(time.RFC3339))
				return nil
			}
			fmt.Fprintln(os.Stderr, "Active Steam account is not paired; waiting without upload.")
			if !pause(ctx, interval) {
				return nil
			}
			continue
		}
		if err = client.ValidateServer(device.Server); err != nil {
			return fmt.Errorf("invalid saved endpoint for this device: %w", err)
		}
		roster, hash, err := readCurrent(coplayDir, account.SteamID64, now())
		if err != nil {
			fmt.Fprintln(os.Stderr, "Waiting for a readable Coplay roster:", err)
			if once {
				return err
			}
			if !pause(ctx, interval) {
				return nil
			}
			continue
		}
		if diagnose || once {
			fmt.Printf("Coplay file ready: %d owner-anchored CS2 peers; owner playtime %s UTC. No upload performed.\n", len(roster.Players), time.Unix(roster.OwnerPlaytime, 0).UTC().Format(time.RFC3339))
			return nil
		}
		if hash != lastHash || now().Sub(lastPost) >= 30*time.Second {
			players := make([]client.Player, 0, len(roster.Players))
			for _, p := range roster.Players {
				players = append(players, client.Player{SteamID64: p.SteamID64, Playtime: p.Playtime, AppID: p.AppID})
			}
			body := client.Snapshot{SourceSteamID64: account.SteamID64, LocalSteamID64: account.SteamID64, ObservedAt: now().UTC(), OwnerPlaytime: roster.OwnerPlaytime, Players: players, ClientVersion: client.Version}
			ctx2, cancel := context.WithTimeout(ctx, 15*time.Second)
			if upload != nil {
				err = upload(ctx2, device.Server, device.Token, body)
			} else {
				err = client.New(device.Server).Snapshot(ctx2, device.Token, body)
			}
			cancel()
			if err != nil {
				fmt.Fprintln(os.Stderr, "Upload failed; will retry:", err)
			} else {
				lastHash, lastPost = hash, now()
				fmt.Printf("Roster updated: %d CS2 peers.\n", len(players))
			}
		}
		if !pause(ctx, interval) {
			return nil
		}
	}
}

func fingerprintCoplayFiles(dir string) (map[string]string, error) {
	paths, err := filepath.Glob(filepath.Join(dir, "coplay_*.vdf"))
	if err != nil {
		return nil, err
	}
	out := make(map[string]string, len(paths))
	for _, name := range paths {
		before, err := os.Stat(name)
		if err != nil {
			if errors.Is(err, fs.ErrNotExist) {
				continue
			}
			return nil, err
		}
		if before.Size() <= 0 || before.Size() > 8<<20 {
			continue
		}
		data, err := os.ReadFile(name)
		if err != nil {
			if errors.Is(err, fs.ErrNotExist) {
				continue
			}
			return nil, err
		}
		after, err := os.Stat(name)
		if err != nil {
			if errors.Is(err, fs.ErrNotExist) {
				continue
			}
			return nil, err
		}
		if before.Size() != after.Size() || !before.ModTime().Equal(after.ModTime()) || int64(len(data)) != after.Size() {
			continue
		}
		sum := sha256.Sum256(data)
		out[name] = hex.EncodeToString(sum[:])
	}
	return out, nil
}

func sameFingerprints(a, b map[string]string) bool {
	if len(a) != len(b) {
		return false
	}
	for p, h := range a {
		if b[p] != h {
			return false
		}
	}
	return true
}

func readCurrent(dir, owner string, now time.Time) (steam.Roster, string, error) {
	files, e := filepath.Glob(filepath.Join(dir, "coplay_*.vdf"))
	if e != nil {
		return steam.Roster{}, "", e
	}
	target := filepath.Join(dir, "coplay_"+owner+".vdf")
	for _, f := range files {
		if !strings.EqualFold(filepath.Clean(f), filepath.Clean(target)) {
			continue
		}
		before, e := os.Stat(f)
		if e != nil {
			return steam.Roster{}, "", e
		}
		data, e := os.ReadFile(f)
		if e != nil {
			return steam.Roster{}, "", e
		}
		after, e := os.Stat(f)
		if e != nil {
			return steam.Roster{}, "", e
		}
		if before.Size() != after.Size() || !before.ModTime().Equal(after.ModTime()) || int64(len(data)) != after.Size() {
			return steam.Roster{}, "", errors.New("file changed while reading")
		}
		r, e := steam.ParseCoplay(data, owner, now)
		if e != nil {
			return steam.Roster{}, "", e
		}
		canonical, _ := json.Marshal(r)
		h := sha256.Sum256(canonical)
		return r, hex.EncodeToString(h[:]), nil
	}
	return steam.Roster{}, "", errors.New("active account Coplay file not found")
}
func pause(ctx context.Context, d time.Duration) bool {
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return false
	case <-t.C:
		return true
	}
}
