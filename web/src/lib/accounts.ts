import type { Schemas } from './api/index.ts';
import { t } from './i18n.svelte.ts';
import { session } from './session.svelte.ts';

type Account = Schemas['Account'];

/**
 * Where money for an invoice (`in`) or a receipt (`out`) moves, the usual one first. Simple books
 * offer the owner's private account, booked as a private withdrawal or deposit, and the bank only
 * once a business account is set up (ADR 0028).
 */
export function moneyAccounts(accounts: Account[], direction: 'in' | 'out'): Account[] {
	const view = session.view;
	const roles = view?.simple_books
		? [
				...(view.business_bank ? ['bank'] : []),
				direction === 'in' ? 'private_withdrawals' : 'private_deposits',
				'cash'
			]
		: ['bank', 'cash', 'money_transit'];
	return roles.flatMap((role) => accounts.filter((a) => a.role === role && !a.archived));
}

export function accountLabel(account: Account): string {
	if (!session.view?.simple_books) return `${account.number} ${account.name}`;
	if (account.role === 'private_withdrawals' || account.role === 'private_deposits') return t('simple.private_account');
	if (account.role === 'bank') return t('simple.business_account');
	return account.name;
}
