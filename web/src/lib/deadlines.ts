import type { Schemas } from './api/index.ts';
import { i18n, t } from './i18n.svelte.ts';

export const monthName = (month: number) =>
	new Date(2000, month - 1, 1).toLocaleDateString(i18n.locale, { month: 'long' });

/** A deadline's title in the UI language, from its kind and period. */
export function deadlineTitle(deadline: Schemas['Deadline']): string {
	const year = Number(deadline.from.slice(0, 4));
	const first = Number(deadline.from.slice(5, 7));
	const last = Number(deadline.to.slice(5, 7));
	const period =
		first === last
			? `${monthName(first)} ${year}`
			: last - first === 2
				? `${t('tax.quarter', { quarter: last / 3 })} ${year}`
				: String(year);
	return t(`tax.deadline.${deadline.kind}`, { period });
}

/** Whole days from today until an ISO date, negative once it has passed. */
export function daysUntil(iso: string): number {
	const [y, m, d] = iso.split('-').map(Number);
	const now = new Date();
	const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
	return Math.round((new Date(y, m - 1, d).getTime() - start.getTime()) / 86_400_000);
}
