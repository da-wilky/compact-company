package app_test

import (
	"strings"
	"testing"
	"time"
)

func wantStartupError(t *testing.T, configTOML, wantInError string) {
	t.Helper()
	a, err := startApp(t, configTOML, &clock{now: time.Now()})
	if err == nil {
		_ = a.Close()
		t.Fatalf("startup succeeded, want error containing %q", wantInError)
	}
	if !strings.Contains(err.Error(), wantInError) {
		t.Fatalf("startup error = %q, want it to contain %q", err, wantInError)
	}
}

func TestUnknownConfigKeyFailsStartup(t *testing.T) {
	wantStartupError(t, devConfig+`sesion_idle_timeout = "1h"`+"\n", "auth.sesion_idle_timeout")
}

func TestInlineClientSecretIsRejected(t *testing.T) {
	wantStartupError(t, oidcConfig(t)+`client_secret = "s3cret"`+"\n", "auth.client_secret")
}

func TestMissingClientSecretFileFailsStartup(t *testing.T) {
	cfg := strings.Replace(oidcConfig(t), "client-secret", "missing", 1)
	wantStartupError(t, cfg, "client_secret_file")
}

func TestOIDCModeRequiresIssuerAndClientID(t *testing.T) {
	wantStartupError(t, strings.Replace(oidcConfig(t), "\nissuer = ", "\n# ", 1), "auth.issuer")
	wantStartupError(t, strings.Replace(oidcConfig(t), "\nclient_id = ", "\n# ", 1), "auth.client_id")
}

func TestOIDCModeRequiresPublicURL(t *testing.T) {
	wantStartupError(t, strings.Replace(oidcConfig(t), "\npublic_url = ", "\n# ", 1), "public_url")
	wantStartupError(t, strings.Replace(oidcConfig(t), `public_url = "https://compact.test"`, `public_url = "compact.test"`, 1), "public_url")
}

func TestUnreachableIssuerFailsStartup(t *testing.T) {
	iss := newIssuer(t)
	cfg := iss.config(t, "alice")
	iss.Close()
	wantStartupError(t, cfg, "auth.issuer")
}

func TestOIDCModeStartsOnAnyAddress(t *testing.T) {
	newTestApp(t, oidcConfig(t))
}

func TestAuthModeMustBeKnown(t *testing.T) {
	wantStartupError(t, "[auth]\nmode = \"password\"\n", "auth.mode")
	wantStartupError(t, "listen = \"127.0.0.1:8080\"\n", "auth.mode")
}

func TestDevModeRefusedOnNonLoopbackAddress(t *testing.T) {
	for _, listen := range []string{"0.0.0.0:8080", ":8080", "192.168.1.10:8080", "example.com:8080"} {
		t.Run(listen, func(t *testing.T) {
			wantStartupError(t, "listen = \""+listen+"\"\n[auth]\nmode = \"none\"\n", "loopback")
		})
	}
}

func TestDevModeAcceptedOnLoopbackAddress(t *testing.T) {
	for _, listen := range []string{"127.0.0.1:8080", "[::1]:8080", "localhost:8080"} {
		t.Run(listen, func(t *testing.T) {
			newTestApp(t, "listen = \""+listen+"\"\n[auth]\nmode = \"none\"\n")
		})
	}
}

func TestInvalidDefaultsFailStartup(t *testing.T) {
	wantStartupError(t, devConfig+"[defaults]\nlanguage = \"fr\"\n", "defaults.language")
	wantStartupError(t, devConfig+"[defaults]\ntimezone = \"Mars/Olympus\"\n", "defaults.timezone")
	wantStartupError(t, devConfig+"[defaults]\ncurrency = \"XYZ\"\n", "defaults.currency")
}
