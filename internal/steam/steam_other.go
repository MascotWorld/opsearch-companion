//go:build !windows

package steam

import "errors"

type Account struct {
	SteamID64 string
	SteamRoot string
}

func ActiveAccount() (Account, error) {
	return Account{}, errors.New("the Coplay companion currently requires Windows")
}
