import de from '#i18n/de.json';
import en from '#i18n/en.json';

const catalogs = { de, en };
export type Locale = keyof typeof catalogs;
export type Key = keyof typeof en;

export const i18n = $state({
	locale: (navigator.language.startsWith('de') ? 'de' : 'en') as Locale
});

export function t(key: Key | string): string {
	return (catalogs[i18n.locale] as Record<string, string>)[key] ?? key;
}
