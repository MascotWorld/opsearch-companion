package app

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
)

type Device struct {
	DeviceID string `json:"device_id"`
	Token    string `json:"token"`
	Server   string `json:"server"`
}
type Config struct {
	Devices map[string]Device `json:"devices"`
}

func ConfigPath() (string, error) {
	base := os.Getenv("LOCALAPPDATA")
	if base == "" {
		return "", errors.New("LOCALAPPDATA is not set")
	}
	return filepath.Join(base, "OPSearch", "Companion", "config.json"), nil
}
func LoadConfig(path string) (Config, error) {
	b, e := os.ReadFile(path)
	if e != nil {
		return Config{}, e
	}
	var c Config
	if e = json.Unmarshal(b, &c); e != nil {
		return c, e
	}
	if c.Devices == nil {
		c.Devices = map[string]Device{}
	}
	return c, nil
}
func SaveConfig(path string, c Config) error {
	if e := os.MkdirAll(filepath.Dir(path), 0700); e != nil {
		return e
	}
	b, e := json.MarshalIndent(c, "", "  ")
	if e != nil {
		return e
	}
	tmp := path + ".tmp"
	if e = os.WriteFile(tmp, b, 0600); e != nil {
		return e
	}
	if e = os.Rename(tmp, path); e != nil {
		_ = os.Remove(tmp)
		return fmt.Errorf("replace companion config: %w", e)
	}
	return nil
}
