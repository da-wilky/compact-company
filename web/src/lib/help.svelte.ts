import type { Schemas } from '#lib/api/index.ts';

export const terms = [
	'taxation',
	'small_business',
	'vat_period',
	'tax_ids',
	'datev',
	'chart',
	'books_start',
	'euer',
	'euer_line',
	'receipt',
	'asset',
	'depreciation',
	'gwg',
	'withdrawal',
	'restricted',
	'partial_invoice',
	'cancellation',
	'einvoice',
	'open_items',
	'dunning',
	'deadlines',
	'year_closing',
	'retention',
	'process_documentation'
] as const;
export type Term = (typeof terms)[number];

type Role = Schemas['Role'];
const books: Role[] = ['owner', 'accountant'];
export const guides: { key: string; roles: Role[]; links: (string | null)[] }[] = [
	{ key: 'setup', roles: ['owner'], links: ['/business', '/layouts', '/books', '/bank', '/process-documentation'] },
	{ key: 'invoice', roles: ['owner'], links: ['/customers', '/documents', null, null, '/open-items'] },
	{ key: 'receipt', roles: books, links: ['/receipts', null, '/assets', '/bank'] },
	{ key: 'bank', roles: books, links: [null, '/bank', null, '/open-items'] },
	{ key: 'asset', roles: books, links: ['/receipts', '/assets', null, null] },
	{ key: 'euer', roles: books, links: ['/bank', '/assets', '/tax?tab=euer', null, '/tax?tab=deadlines'] },
	{ key: 'year', roles: books, links: [null, '/assets', '/tax?tab=exports'] },
	{ key: 'time', roles: ['owner', 'staff'], links: ['/todos', '/time', null] }
];

export const explaining = $state<{ term?: Term }>({});
