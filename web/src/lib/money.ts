import { i18n } from './i18n.svelte.ts';

const digits = (currency: string) =>
	new Intl.NumberFormat('en', { style: 'currency', currency }).resolvedOptions()
		.maximumFractionDigits ?? 2;

/** An amount in minor units, formatted for the current language. */
export function formatMoney(minor: number, currency: string): string {
	return new Intl.NumberFormat(i18n.locale, { style: 'currency', currency }).format(
		minor / 10 ** digits(currency)
	);
}

/** The amount as the user would type it, without currency sign or grouping. */
export function moneyInput(minor: number | null | undefined, currency: string): string {
	if (minor == null) return '';
	return new Intl.NumberFormat(i18n.locale, {
		minimumFractionDigits: digits(currency),
		useGrouping: false
	}).format(minor / 10 ** digits(currency));
}

/**
 * Reads a typed amount into minor units: `null` when empty, `NaN` when it is not an amount.
 * Grouping and decimal marks follow the current language, so "1.234,50" is valid in German.
 */
export function parseMoney(text: string, currency: string): number | null {
	const [group, decimal] = i18n.locale === 'de' ? ['.', ','] : [',', '.'];
	const plain = text.replace(/[\s €$£]|[A-Z]{3}/g, '').split(group).join('');
	if (!plain) return null;
	const places = digits(currency);
	const pattern = new RegExp(`^\\d+(\\${decimal}\\d{0,${places}})?$`);
	if (!pattern.test(plain)) return NaN;
	const [whole, fraction = ''] = plain.split(decimal);
	return Number(whole) * 10 ** places + Number(fraction.padEnd(places, '0') || 0);
}
