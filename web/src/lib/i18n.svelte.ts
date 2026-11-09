import de from '#i18n/de.json';
import en from '#i18n/en.json';

const catalogs = { de, en };
export type Locale = keyof typeof catalogs;

export const i18n = $state({
	locale: (navigator.language.startsWith('de') ? 'de' : 'en') as Locale
});

/** Translates a catalog key, filling `{name}` placeholders from `params`. */
export function t(key: string, params: Record<string, string | number> = {}): string {
	const text = (catalogs[i18n.locale] as Record<string, string>)[key] ?? key;
	return text.replace(/\{(\w+)\}/g, (_, name) => String(params[name] ?? `{${name}}`));
}

export function has(key: string): boolean {
	return key in catalogs.en;
}
