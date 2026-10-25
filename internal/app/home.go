package app

import "net/http"

func init() {
	routes = append(routes, func(a *App, mux *http.ServeMux) {
		mux.HandleFunc("GET /{$}", a.authed(a.home))
	})
}

func (a *App) home(w http.ResponseWriter, r *http.Request) {
	a.render(w, r, http.StatusOK, "home", nil)
}
