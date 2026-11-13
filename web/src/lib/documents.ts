import type { Schemas } from '#lib/api/index.ts';
import { i18n, t } from '#lib/i18n.svelte.ts';

export type Kind = Schemas['DocumentKind'];

export const kinds: Kind[] = [
	'quote',
	'order_confirmation',
	'invoice',
	'cancellation',
	'delivery_note',
	'acceptance_report',
	'dunning'
];

const terms: Record<Kind, string> = {
	quote: 'term.quote',
	order_confirmation: 'term.order_confirmation',
	invoice: 'term.invoice',
	cancellation: 'term.cancellation_invoice',
	delivery_note: 'term.delivery_note',
	acceptance_report: 'term.acceptance_report',
	dunning: 'term.dunning_letter'
};

export const kindLabel = (kind: Kind) => t(terms[kind]);

/** A document's kind, naming a partial or final invoice as such. */
export const docLabel = (kind: Kind, invoiceType?: Schemas['InvoiceType'] | null) =>
	invoiceType ? t(`term.${invoiceType}_invoice`) : kindLabel(kind);

/** An amount in major units as the API sends it (`"1234.50"`), formatted for the current language. */
export function formatAmount(major: string | null | undefined, currency: string): string {
	if (major == null) return '–';
	return new Intl.NumberFormat(i18n.locale, { style: 'currency', currency }).format(Number(major));
}

/** A decimal as the user would type it, `2,5` in German. */
export function decimalInput(value: string | null | undefined): string {
	if (value == null) return '';
	return i18n.locale === 'de' ? value.replace('.', ',') : value;
}

/** Reads a typed decimal into the API's form (`2.5`): `null` when empty, `NaN` when invalid. */
export function parseDecimal(text: string): string | null | typeof NaN {
	const plain = text.trim().replace(/\s/g, '');
	if (!plain) return null;
	const normal = i18n.locale === 'de' ? plain.replace(/\./g, '').replace(',', '.') : plain.replace(/,/g, '');
	return /^-?\d+(\.\d+)?$/.test(normal) ? normal : NaN;
}

export const units: Schemas['Unit'][] = ['hour', 'day', 'month', 'piece', 'lump_sum'];

/** What a missing or unshown field means for the person issuing the document. */
export function missingLabel(key: string): string {
	const line = key.match(/^lines\.(\d+)\.(\w+)$/);
	if (line) return t(`documents.missing.line_${line[2]}`, { n: Number(line[1]) + 1 });
	const deduction = key.match(/^deductions\.(\d+)\.\w+$/);
	if (deduction) return t('documents.missing.deduction', { n: Number(deduction[1]) + 1 });
	const group = key.split('.')[0];
	if (group === 'business' || group === 'customer') {
		return t(`documents.missing.${group}`, { field: t(`${group === 'business' ? 'business' : 'customers'}.${key.split('.')[1]}`) });
	}
	return t(`documents.missing.${key.replace('.', '_')}`);
}
