import { i18n, type Locale } from './i18n.svelte.ts';

export type Theme = 'system' | 'light' | 'dark';

function stored(key: string): string | null {
	try {
		return localStorage.getItem(key);
	} catch {
		return null;
	}
}

function store(key: string, value: string) {
	try {
		localStorage.setItem(key, value);
	} catch {
		// private windows may refuse storage; the choice then lasts for this visit
	}
}

export const preferences = $state({
	theme: (stored('theme') ?? 'system') as Theme
});

const locale = stored('locale');
if (locale === 'de' || locale === 'en') i18n.locale = locale;

export function setTheme(theme: Theme) {
	preferences.theme = theme;
	store('theme', theme);
	applyTheme();
}

export function setLocale(locale: Locale) {
	i18n.locale = locale;
	store('locale', locale);
}

export function applyTheme() {
	const root = document.documentElement;
	if (preferences.theme === 'system') delete root.dataset.theme;
	else root.dataset.theme = preferences.theme;
}
