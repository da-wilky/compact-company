<script lang="ts">
	import { api, call, clientError, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { decimalInput, parseDecimal, units } from '#lib/documents.ts';
	import { formatDay } from '#lib/dates.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { moneyInput, parseMoney } from '#lib/money.ts';
	import { session } from '#lib/session.svelte.ts';

	type Recurring = Schemas['Recurring'];
	type EditLine = { description: string; quantity: string; unit: Schemas['Unit']; price: string; vat: string };

	const owner = $derived(session.view?.user?.role === 'owner');
	let list = $state<Recurring[]>([]);
	let error = $state<ApiError>();
	let customers = $state<Schemas['Customer'][]>([]);
	let projects = $state<Schemas['Project'][]>([]);

	async function load() {
		const result = await call(api.GET('/api/recurring-invoices'));
		list = result.data ?? [];
		error = result.error;
	}
	$effect(() => {
		load();
	});

	let open = $state(false);
	let editing = $state<number>();
	let customerId = $state<number>();
	let projectId = $state<number | null>(null);
	let title = $state('');
	let interval = $state<Schemas['Interval']>('monthly');
	let period = $state<Schemas['Period']>('advance');
	let nextDate = $state('');
	let endsOn = $state('');
	let paused = $state(false);
	let lines = $state<EditLine[]>([]);
	let formError = $state<ApiError>();

	const currency = $derived(customers.find((c) => c.id === customerId)?.currency ?? 'EUR');

	$effect(() => {
		const customer_id = customerId;
		if (!customer_id) return;
		call(api.GET('/api/projects', { params: { query: { customer_id } } })).then(
			({ data }) => (projects = data ?? [])
		);
	});

	async function edit(item?: Recurring) {
		if (!customers.length) customers = (await call(api.GET('/api/customers'))).data ?? [];
		editing = item?.id;
		customerId = item?.customer_id ?? customers[0]?.id;
		projectId = item?.project_id ?? null;
		title = item?.title ?? '';
		interval = item?.interval ?? 'monthly';
		period = item?.period ?? 'advance';
		nextDate = item?.next_date ?? new Date().toISOString().slice(0, 10);
		endsOn = item?.ends_on ?? '';
		paused = item?.paused ?? false;
		const lineCurrency = customers.find((c) => c.id === customerId)?.currency ?? 'EUR';
		lines = item
			? item.lines.map((l) => ({
					description: l.description,
					quantity: decimalInput(l.quantity),
					unit: l.unit,
					price: moneyInput(l.unit_price, lineCurrency),
					vat: decimalInput(l.vat_rate)
				}))
			: [{ description: '', quantity: '1', unit: 'month', price: '', vat: '' }];
		formError = undefined;
		open = true;
	}

	function body(): Schemas['RecurringInput'] | undefined {
		if (!customerId) return;
		const parsed = lines.map((l) => ({
			description: l.description,
			quantity: parseDecimal(l.quantity),
			unit: l.unit,
			unit_price: parseMoney(l.price, currency),
			vat_rate: parseDecimal(l.vat)
		}));
		const fields: Record<string, Schemas['FieldError']> = {};
		parsed.forEach((l, i) => {
			if (l.quantity == null || Number.isNaN(l.quantity)) fields[`lines.${i}.quantity`] = 'invalid_format';
			if (Number.isNaN(l.unit_price)) fields[`lines.${i}.unit_price`] = 'invalid_format';
			if (Number.isNaN(l.vat_rate)) fields[`lines.${i}.vat_rate`] = 'invalid_format';
		});
		if (Object.keys(fields).length) {
			formError = { ...clientError('error.invalid_input'), fields };
			return;
		}
		return {
			customer_id: customerId,
			project_id: projectId,
			title: title || null,
			interval,
			period,
			next_date: nextDate,
			ends_on: endsOn || null,
			paused,
			lines: parsed as Schemas['LineInput'][]
		};
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const input = body();
		if (!input) return;
		const result = editing
			? await call(api.PUT('/api/recurring-invoices/{id}', { params: { path: { id: editing } }, body: input }))
			: await call(api.POST('/api/recurring-invoices', { body: input }));
		formError = result.error;
		if (result.data) {
			open = false;
			load();
		}
	}

	async function remove() {
		if (!editing || !confirm(t('recurring.delete_confirm'))) return;
		const result = await call(api.DELETE('/api/recurring-invoices/{id}', { params: { path: { id: editing } } }));
		formError = result.error;
		if (!result.error) {
			open = false;
			load();
		}
	}

	const fieldError = (key: string) => formError?.fields?.[key] as Schemas['FieldError'] | undefined;
</script>

<div class="page-head">
	<div>
		<p class="muted eyebrow"><a href="/documents">{t('documents.title')}</a></p>
		<h1>{t('recurring.title')}</h1>
	</div>
	{#if owner}
		<button class="primary" onclick={() => edit()}><Icon name="plus" size={18} />{t('recurring.new')}</button>
	{/if}
</div>

<div class="stack">
	<p class="muted intro">{t('recurring.hint')}</p>
	{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
	{#if list.length}
		<ul class="list card-list">
			{#each list as item (item.id)}
				<li>
					<button class="row" disabled={!owner} onclick={() => edit(item)}>
						<span class="main">
							<strong>{item.title ?? item.lines[0]?.description ?? t('term.invoice')}</strong>
							<small class="muted">
								{item.customer_name} · {t(`recurring.interval.${item.interval}`)} · {t(`recurring.period.${item.period}`)}
							</small>
						</span>
						<span class="side">
							{#if item.paused}
								<small class="badge">{t('recurring.paused')}</small>
							{:else if item.next_date}
								<small class="muted">{t('recurring.next')} {formatDay(item.next_date, { dateStyle: 'medium' })}</small>
							{:else}
								<small class="badge">{t('recurring.ended')}</small>
							{/if}
						</span>
					</button>
					{#if item.invoices.length}
						<div class="produced">
							{#each item.invoices.slice(0, 6) as invoice (invoice.id)}
								<a href={`/documents/${invoice.id}`}>{invoice.number ?? `${t('term.draft')} ${formatDay(invoice.recurring_date, { dateStyle: 'short' })}`}</a>
							{/each}
						</div>
					{/if}
				</li>
			{/each}
		</ul>
	{:else}
		<p class="card empty">{t('recurring.none')}</p>
	{/if}
</div>

<Dialog bind:open title={t(editing ? 'recurring.edit' : 'recurring.new')}>
	<form class="stack" onsubmit={save}>
		<Field label={t('term.customer')} error={fieldError('customer_id')}>
			<select bind:value={customerId}>
				{#each customers as customer (customer.id)}<option value={customer.id}>{customer.name}</option>{/each}
			</select>
		</Field>
		<Field label={t('term.project')} error={fieldError('project_id')}>
			<select bind:value={projectId}>
				<option value={null}>–</option>
				{#each projects as project (project.id)}<option value={project.id}>{project.name}</option>{/each}
			</select>
		</Field>
		<Field label={t('documents.doc_title')} error={fieldError('title')}>
			<input bind:value={title} />
		</Field>
		<Segmented
			label={t('recurring.interval_label')}
			options={[
				{ value: 'monthly', label: t('recurring.interval.monthly') },
				{ value: 'quarterly', label: t('recurring.interval.quarterly') },
				{ value: 'yearly', label: t('recurring.interval.yearly') }
			]}
			value={interval}
			onchange={(v) => (interval = v)}
		/>
		<Segmented
			label={t('term.service_period')}
			options={[
				{ value: 'advance', label: t('recurring.period.advance') },
				{ value: 'arrears', label: t('recurring.period.arrears') }
			]}
			value={period}
			onchange={(v) => (period = v)}
		/>
		<div class="pair">
			<Field label={t('recurring.next_date')} error={fieldError('next_date')}>
				<input type="date" bind:value={nextDate} required />
			</Field>
			<Field label={t('recurring.ends_on')} error={fieldError('ends_on')}>
				<input type="date" bind:value={endsOn} />
			</Field>
		</div>
		<h3>{t('documents.lines')}</h3>
		{#each lines as line, i (i)}
			<div class="line">
				<div class="wide">
					<Field label={t('documents.description')} error={fieldError(`lines.${i}.description`)}>
						<input bind:value={line.description} />
					</Field>
				</div>
				<Field label={t('documents.quantity')} error={fieldError(`lines.${i}.quantity`)}>
					<input inputmode="decimal" bind:value={line.quantity} />
				</Field>
				<Field label={t('documents.unit')}>
					<select bind:value={line.unit}>
						{#each units as unit (unit)}<option value={unit}>{t(`documents.unit.${unit}`)}</option>{/each}
					</select>
				</Field>
				<Field label={t('documents.unit_price')} error={fieldError(`lines.${i}.unit_price`)} note={currency}>
					<input inputmode="decimal" bind:value={line.price} />
				</Field>
				<Field label={t('documents.vat')} error={fieldError(`lines.${i}.vat_rate`)}>
					<input inputmode="decimal" bind:value={line.vat} />
				</Field>
				<div class="line-actions">
					<button type="button" class="ghost icon" aria-label={t('app.remove')} disabled={lines.length === 1} onclick={() => lines.splice(i, 1)}><Icon name="close" size={18} /></button>
				</div>
			</div>
		{/each}
		<div>
			<button type="button" class="ghost" onclick={() => lines.push({ description: '', quantity: '1', unit: 'month', price: '', vat: '' })}>
				<Icon name="plus" size={18} />{t('documents.add_line')}
			</button>
		</div>
		{#if editing}
			<Field label={t('recurring.paused')} inline>
				<input type="checkbox" bind:checked={paused} />
			</Field>
		{/if}
		{#if formError && !formError.fields}<p class="alert" role="alert">{t(formError.message)}</p>{/if}
		<div class="buttons">
			<button class="primary">{t('app.save')}</button>
			{#if editing}<button type="button" class="ghost" onclick={() => { endsOn = new Date().toISOString().slice(0, 10); }}>{t('recurring.end')}</button>{/if}
			{#if editing}<button type="button" class="ghost" onclick={remove}>{t('app.delete')}</button>{/if}
		</div>
	</form>
</Dialog>

<style>
	.eyebrow {
		margin: 0;
		font-size: 0.875rem;
	}
	.intro {
		max-width: 64rem;
		margin: 0;
	}
	.card-list {
		max-width: 64rem;
		background: var(--surface);
	}
	.card-list li {
		padding: 0;
		display: grid;
	}
	.row {
		display: flex;
		justify-content: space-between;
		gap: 1rem;
		padding: 0.875rem 1rem;
		color: inherit;
		background: none;
		border: 0;
		border-radius: 0;
		font: inherit;
		text-align: left;
		width: 100%;
		min-width: 0;
	}
	.row:hover:not(:disabled) {
		background: var(--surface-2);
	}
	.row:disabled {
		opacity: 1;
		cursor: default;
	}
	.main {
		display: grid;
		min-width: 0;
	}
	.main small {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.side {
		display: flex;
		flex-wrap: wrap;
		justify-content: end;
		align-items: center;
		gap: 0.25rem 0.75rem;
		text-align: right;
	}
	.produced {
		display: flex;
		flex-wrap: wrap;
		gap: 0.25rem 1rem;
		padding: 0 1rem 0.875rem;
		font-size: 0.875rem;
	}
	h3 {
		margin: 0.5rem 0 0;
		font-size: 1rem;
	}
	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 0.75rem;
	}
	.line {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(7rem, 1fr));
		gap: 0.75rem;
		padding: 0.75rem;
		border: 1px solid var(--border);
		border-radius: var(--radius, 0.5rem);
		align-items: end;
	}
	.wide {
		grid-column: 1 / -1;
	}
	.line-actions {
		display: flex;
		justify-content: end;
	}
	.buttons {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}
</style>
