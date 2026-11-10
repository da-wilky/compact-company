import type { Schemas } from '#lib/api/index.ts';
import { t } from '#lib/i18n.svelte.ts';

export type Kind = Schemas['DocumentKind'];

export const kinds: Kind[] = [
	'quote',
	'order_confirmation',
	'invoice',
	'cancellation',
	'delivery_note',
	'acceptance_report'
];

const terms: Record<Kind, string> = {
	quote: 'term.quote',
	order_confirmation: 'term.order_confirmation',
	invoice: 'term.invoice',
	cancellation: 'term.cancellation_invoice',
	delivery_note: 'term.delivery_note',
	acceptance_report: 'term.acceptance_report'
};

export const kindLabel = (kind: Kind) => t(terms[kind]);
