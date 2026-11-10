import { i18n, t } from './i18n.svelte.ts';

/** Today as `YYYY-MM-DD` in the browser's time zone. */
export function today(): string {
	return isoDay(new Date());
}

export function isoDay(date: Date): string {
	const pad = (n: number) => String(n).padStart(2, '0');
	return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

export function formatDay(day: string, options: Intl.DateTimeFormatOptions = { day: '2-digit', month: '2-digit' }): string {
	const [y, m, d] = day.split('-').map(Number);
	return new Date(y, m - 1, d).toLocaleDateString(i18n.locale, options);
}

/** "due 12.11." with whether it has passed; nothing without a due date. */
export function dueLabel(day: string | null | undefined, done: boolean) {
	if (!day) return undefined;
	return { text: t('todos.due', { day: formatDay(day) }), overdue: !done && day < today() };
}
