package app

import (
	"bytes"
	"context"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	typsttemplates "github.com/da-wilky/compact-company/templates"
)

// extractDefaultTemplate writes the bundled template under dataDir, replacing any
// earlier copy, because typst reads templates from disk.
func extractDefaultTemplate(dataDir string) (string, error) {
	dir := filepath.Join(dataDir, "default-template")
	if err := os.RemoveAll(dir); err != nil {
		return "", err
	}
	sub, err := fs.Sub(typsttemplates.Default, "default")
	if err != nil {
		return "", err
	}
	return dir, os.CopyFS(dir, sub)
}

// typstError is a failed render, shown to the user as typst reported it.
type typstError struct{ message string }

func (e *typstError) Error() string { return "typst: " + e.message }

// renderPDF compiles the template's invoice.typ with data as sys.inputs.data (contract version 1).
func (a *App) renderPDF(ctx context.Context, data []byte) ([]byte, error) {
	out, err := os.MkdirTemp("", "compact-company-render-")
	if err != nil {
		return nil, err
	}
	defer func() { _ = os.RemoveAll(out) }()
	pdf := filepath.Join(out, "invoice.pdf")
	args := []string{"compile", "--root", a.templateDir}
	for _, p := range a.cfg.Invoice.FontPaths {
		args = append(args, "--font-path", p)
	}
	args = append(args, "--input", "data="+string(data), filepath.Join(a.templateDir, "invoice.typ"), pdf)
	cmd := exec.CommandContext(ctx, "typst", args...)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		msg := strings.TrimSpace(stderr.String())
		if msg == "" {
			msg = err.Error()
		}
		return nil, &typstError{message: msg}
	}
	return os.ReadFile(pdf)
}
