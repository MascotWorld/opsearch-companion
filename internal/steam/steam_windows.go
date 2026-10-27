//go:build windows

package steam

import (
	"errors"
	"fmt"
	"path/filepath"
	"strconv"

	"golang.org/x/sys/windows/registry"
)

const steam64Base uint64 = 76561197960265728

type Account struct {
	SteamID64 string
	SteamRoot string
}

// ActiveAccount reads only Steam's per-user ActiveProcess registry values. It
// does not inspect or open any game process.
func ActiveAccount() (Account, error) {
	k, e := registry.OpenKey(registry.CURRENT_USER, `Software\Valve\Steam\ActiveProcess`, registry.QUERY_VALUE)
	if e != nil {
		return Account{}, fmt.Errorf("Steam ActiveProcess registry key unavailable: %w", e)
	}
	defer k.Close()
	account, _, e := k.GetIntegerValue("ActiveUser")
	if e != nil || account == 0 {
		return Account{}, errors.New("Steam is not signed in for the current Windows user")
	}
	rootKey, e := registry.OpenKey(registry.CURRENT_USER, `Software\Valve\Steam`, registry.QUERY_VALUE)
	if e != nil {
		return Account{}, fmt.Errorf("Steam install path registry key unavailable: %w", e)
	}
	defer rootKey.Close()
	root, _, e := rootKey.GetStringValue("SteamPath")
	if e != nil || root == "" {
		return Account{}, errors.New("SteamPath registry value is missing")
	}
	id := steam64Base + account
	return Account{strconv.FormatUint(id, 10), filepath.Clean(root)}, nil
}
