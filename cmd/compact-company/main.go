package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/da-wilky/compact-company/internal/app"
	"github.com/da-wilky/compact-company/internal/config"
)

const usage = "usage: compact-company serve --config <file>"

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "compact-company:", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	if len(args) == 0 || args[0] != "serve" {
		return errors.New(usage)
	}
	flags := flag.NewFlagSet("serve", flag.ContinueOnError)
	configPath := flags.String("config", "", "path to the TOML config file")
	if err := flags.Parse(args[1:]); err != nil {
		return err
	}
	if *configPath == "" || flags.NArg() > 0 {
		return errors.New(usage)
	}
	return serve(*configPath)
}

func serve(configPath string) error {
	cfg, err := config.Load(configPath)
	if err != nil {
		return err
	}
	a, err := app.New(cfg, time.Now)
	if err != nil {
		return err
	}
	defer func() { _ = a.Close() }()

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	srv := &http.Server{Addr: cfg.Listen, Handler: a, ReadHeaderTimeout: 10 * time.Second}
	errc := make(chan error, 1)
	go func() { errc <- srv.ListenAndServe() }()
	slog.Info("listening", "addr", cfg.Listen, "auth", cfg.Auth.Mode)

	select {
	case err := <-errc:
		return err
	case <-ctx.Done():
	}
	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	return srv.Shutdown(shutdownCtx)
}
