<script lang="ts">
	import { api, call, clientError, type ApiError, type Schemas } from '#lib/api/index.ts';
	import { accountLabel, moneyAccounts } from '#lib/accounts.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { formatDay, today } from '#lib/dates.ts';
	import { formatAmount } from '#lib/documents.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { formatMoney, moneyInput, parseMoney } from '#lib/money.ts';
	import { session } from '#lib/session.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	type Invoice = Schemas['DocumentSummary'];

	const owner = $derived(session.view?.user?.role === 'owner');
	let openOnly = $state(true);
	let invoices = $state<Invoice[]>([]);
	let accounts = $state<Schemas['Account'][]>([]);
	let error = $state<ApiError>();

	async function load() {
		const result = await call(api.GET('/api/documents', { params: { query: { kind: 'invoice', status: 'issued' } } }));
		invoices = result.data ?? [];
		error = result.error;
	}
	load();
	call(api.GET('/api/accounts')).then(({ data }) => (accounts = data ?? []));

	const shown = $derived(openOnly ? invoices.filter((i) => (i.amount_due ?? 0) !== 0) : invoices);
	const through = $derived(moneyAccounts(accounts, 'in'));
	const day = (iso: string | null | undefined) => (iso ? formatDay(iso, { dateStyle: 'medium' }) : '–');

	let paying = $state<Invoice>();
	let payOpen = $state(false);
	let payDate = $state(today());
	let payAmount = $state('');
	let payEur = $state('');
	let payAccount = $state<number>();
	let payError = $state<ApiError>();

	function receive(invoice: Invoice) {
		paying = invoice;
		payDate = today();
		payAmount = moneyInput(invoice.amount_due, invoice.currency);
		payEur = '';
		payAccount = through[0]?.id;
		payError = undefined;
		payOpen = true;
	}

	async function record(event: SubmitEvent) {
		event.preventDefault();
		if (!paying) return;
		const amount = parseMoney(payAmount, paying.currency);
		const eur = payEur.trim() ? parseMoney(payEur, 'EUR') : null;
		if (amount == null || Number.isNaN(amount) || Number.isNaN(eur)) {
			payError = { ...clientError('error.invalid_input'), fields: { amount: 'invalid_format' } };
			return;
		}
		const result = await call(
			api.POST('/api/documents/{id}/payments', {
				params: { path: { id: paying.id } },
				body: { date: payDate, amount, eur_amount: eur, account_id: payAccount ?? null }
			})
		);
		payError = result.error;
		if (result.data) {
			payOpen = false;
			toast(t('simple.payment_recorded'));
			await load();
		}
	}
</script>

<div class="page-head">
	<div>
		<h1>{t('simple.income')}</h1>
		<p class="muted">{t('simple.income_intro')}</p>
	</div>
	{#if owner}
		<div class="row">
			<a class="button primary" href="/documents"><Icon name="plus" size={18} />{t('simple.write_invoice')}</a>
		</div>
	{/if}
</div>

<div class="stack">
	<div>
		<Segmented
			label={t('simple.income')}
			options={[
				{ value: 'open', label: t('receipts.open') },
				{ value: 'all', label: t('receipts.all') }
			]}
			value={openOnly ? 'open' : 'all'}
			onchange={(v) => (openOnly = v === 'open')}
		/>
	</div>
	{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
	{#if shown.length}
		<div class="table-wrap stacked">
			<table>
				<thead>
					<tr>
						<th>{t('receipts.date')}</th>
						<th>{t('term.invoice')}</th>
						<th>{t('term.customer')}</th>
						<th class="amount">{t('receipts.gross')}</th>
						<th class="amount">{t('documents.amount_due')}</th>
						{#if owner}<th></th>{/if}
					</tr>
				</thead>
				<tbody>
					{#each shown as invoice (invoice.id)}
						<tr>
							<td data-label={t('receipts.date')}>{day(invoice.issue_date)}</td>
							<td data-label={t('term.invoice')}><a href="/documents/{invoice.id}">{invoice.number}</a></td>
							<td data-label={t('term.customer')}>{invoice.customer_name}</td>
							<td data-label={t('receipts.gross')} class="amount">{formatAmount(invoice.gross, invoice.currency)}</td>
							<td data-label={t('documents.amount_due')} class="amount">
								{#if invoice.amount_due}
									{formatMoney(invoice.amount_due, invoice.currency)}
								{:else}
									<span class="badge accent">{t('simple.settled')}</span>
								{/if}
							</td>
							{#if owner}
								<td class="actions">
									{#if (invoice.amount_due ?? 0) > 0}
										<button class="ghost" onclick={() => receive(invoice)}>{t('simple.receive')}</button>
									{/if}
								</td>
							{/if}
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else}
		<p class="card empty">{t(openOnly ? 'simple.no_open_income' : 'simple.no_income')}</p>
	{/if}
	{#if owner && session.view?.simple_books && !session.view.business_bank}
		<small class="muted">{t('simple.business_account_hint')} <a href="/bank">{t('bank.title')}</a></small>
	{/if}
</div>

<Dialog bind:open={payOpen} title={t('simple.receive')}>
	{#if paying}
		<form class="stack" onsubmit={record}>
			<p class="muted">{paying.number} · {paying.customer_name}</p>
			<Field label={t('simple.received_on')} error={payError?.fields?.date}>
				<input type="date" bind:value={payDate} />
			</Field>
			<Field label={t('documents.payment_amount')} error={payError?.fields?.amount} suffix={paying.currency}>
				<input inputmode="decimal" bind:value={payAmount} />
			</Field>
			{#if paying.currency !== 'EUR'}
				<Field label={t('documents.payment_eur')} error={payError?.fields?.eur_amount} suffix="EUR">
					<input inputmode="decimal" bind:value={payEur} />
				</Field>
			{/if}
			<Field label={t('simple.received_through')} error={payError?.fields?.account_id}>
				<select bind:value={payAccount}>
					{#each through as account (account.id)}<option value={account.id}>{accountLabel(account)}</option>{/each}
				</select>
			</Field>
			{#if payError && !payError.fields}<p class="alert" role="alert">{t(payError.message)}</p>{/if}
			<div><button class="primary">{t('documents.record_payment')}</button></div>
		</form>
	{/if}
</Dialog>

<style>
	.amount {
		text-align: right;
		white-space: nowrap;
		font-variant-numeric: tabular-nums;
	}
	.actions {
		text-align: right;
	}
</style>
