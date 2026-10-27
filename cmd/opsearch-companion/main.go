package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/MascotWorld/opsearch-companion/internal/app"
	"github.com/MascotWorld/opsearch-companion/internal/client"
	"github.com/MascotWorld/opsearch-companion/internal/steam"
)

func main() {
	config := flag.String("config", "", "path to local paired-device config")
	server := flag.String("server", client.DefaultServer, "HTTPS service URL (HTTP is accepted only for localhost testing)")
	pairCode := flag.String("pair", "", "one-time pairing code from your OPSearch Coplay page")
	once := flag.Bool("once", false, "inspect the current roster once locally; never upload")
	diagnose := flag.Bool("diagnose", false, "inspect Steam account and VDF status locally; never upload")
	coplayFile := flag.String("coplay-file", "", "read one Coplay VDF file locally (requires --owner-steamid64 and --diagnose or --once)")
	ownerID := flag.String("owner-steamid64", "", "filename owner SteamID64 when using --coplay-file")
	flag.Parse()
	path := *config
	if err := client.ValidateServer(*server); err != nil {
		fatal(err)
	}
	if *coplayFile != "" {
		if !*diagnose && !*once {
			fatal(errors.New("--coplay-file is read-only and requires --diagnose or --once"))
		}
		if *ownerID == "" {
			fatal(errors.New("--owner-steamid64 is required with --coplay-file"))
		}
		roster, err := app.Inspect(*coplayFile, *ownerID, time.Now())
		if err != nil {
			fatal(err)
		}
		fmt.Printf("Coplay file parsed: %d owner-anchored CS2 peers; owner playtime %s UTC. No upload performed.\n", len(roster.Players), time.Unix(roster.OwnerPlaytime, 0).UTC().Format(time.RFC3339))
		return
	}
	if path == "" {
		var e error
		path, e = app.ConfigPath()
		if e != nil {
			fatal(e)
		}
	}
	if *pairCode != "" {
		if err := pair(path, *server, *pairCode); err != nil {
			fatal(err)
		}
		return
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	if err := app.Run(ctx, path, *once, *diagnose); err != nil {
		fatal(err)
	}
}
func pair(path, server, code string) error {
	account, e := steam.ActiveAccount()
	if e != nil {
		return e
	}
	code = strings.TrimSpace(code)
	if len(code) != 12 {
		return errors.New("pairing code must be exactly 12 characters")
	}
	name, _ := os.Hostname()
	if name == "" {
		name = "Windows PC"
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	result, e := client.New(server).Pair(ctx, client.PairRequest{Code: code, SteamID64: account.SteamID64, DeviceName: name, ClientVersion: client.Version})
	if e != nil {
		return fmt.Errorf("pair device: %w", e)
	}
	cfg, e := app.LoadConfig(path)
	if e != nil && !errors.Is(e, os.ErrNotExist) {
		return fmt.Errorf("read config: %w", e)
	}
	if errors.Is(e, os.ErrNotExist) {
		cfg = app.Config{Devices: map[string]app.Device{}}
	}
	if cfg.Devices == nil {
		cfg.Devices = map[string]app.Device{}
	}
	cfg.Devices[account.SteamID64] = app.Device{DeviceID: result.DeviceID, Token: result.Token, Server: strings.TrimRight(server, "/")}
	if e = app.SaveConfig(path, cfg); e != nil {
		return e
	}
	fmt.Printf("Paired Steam account %s. Credential saved to %s\n", account.SteamID64, path)
	return nil
}
func fatal(e error) { fmt.Fprintln(os.Stderr, "OPSearch Companion:", e); os.Exit(1) }
