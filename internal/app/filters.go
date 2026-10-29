package app

import (
	"maps"
	"net/url"
	"slices"
)

// filterOption is one choice of a filter-seg toggle; Label is a catalog key.
type filterOption struct {
	Label   string
	URL     string
	Current bool
}

// filterSelect is a filter-select: changing it submits Name to Action with Hidden, the other active filters.
type filterSelect struct {
	Action string
	Name   string
	Label  string
	All    string
	Hidden []hiddenField
	Groups []filterGroup
}

// filterGroup is an optgroup, or loose options when Label is "".
type filterGroup struct {
	Label   string
	Options []filterSelectOption
}

// filterSelectOption shows Key from the catalog, else Name.
type filterSelectOption struct {
	Value, Name, Key string
	Archived         bool
	Selected         bool
}

type hiddenField struct {
	Name, Value string
}

func hiddenFields(q url.Values, except string) []hiddenField {
	var fields []hiddenField
	for _, name := range slices.Sorted(maps.Keys(q)) {
		if name != except {
			fields = append(fields, hiddenField{name, q.Get(name)})
		}
	}
	return fields
}
