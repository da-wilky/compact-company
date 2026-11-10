<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, call, clientError, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Dialog from '#lib/components/Dialog.svelte';
	import Field from '#lib/components/Field.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import {
		decimalInput,
		formatAmount,
		kindLabel,
		missingLabel,
		parseDecimal,
		units,
		type Kind
	} from '#lib/documents.ts';
	import { formatDay } from '#lib/dates.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import { formatMoney, moneyInput, parseMoney } from '#lib/money.ts';
	import { session } from '#lib/session.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	type Doc = Schemas['Document'];
	type EditLine = {
		description: string;
		quantity: string;
		unit: Schemas['Unit'];
		price: string;
		vat: string;
		project_id: number | null;
		activity_id: number | null;
		time_entry_ids: number[];
		cancels_line_id: number | null;
	};

	const id = $derived(Number(page.params.id));
	const owner = $derived(session.view?.user?.role === 'owner');
	let doc = $state<Doc>();
	let error = $state<ApiError>();
	let busy = $state(false);
	let issueError = $state<ApiError>();

	let customerId = $state(0);
	let projectId = $state<number | null>(null);
	let language = $state<Schemas['Language']>('de');
	let title = $state('');
	let intro = $state('');
	let outro = $state('');
	let validUntil = $state('');
	let pricing = $state<Schemas['Pricing']>('fixed');
	let serviceFrom = $state('');
	let serviceTo = $state('');
	let timesheet = $state(false);
	let lines = $state<EditLine[]>([]);
	let dirty = $state(false);

	let customers = $state<Schemas['Customer'][]>([]);
	let projects = $state<Schemas['Project'][]>([]);
	let activities = $state<Schemas['Activity'][]>([]);
	let snippets = $state<Schemas['Snippet'][]>([]);

	function show(next: Doc) {
		doc = next;
		customerId = next.customer_id;
		projectId = next.project_id ?? null;
		language = next.language;
		title = next.title ?? '';
		intro = next.intro ?? '';
		outro = next.outro ?? '';
		validUntil = next.valid_until ?? '';
		pricing = next.pricing ?? 'fixed';
		serviceFrom = next.service_from ?? '';
		serviceTo = next.service_to ?? '';
		timesheet = next.timesheet;
		lines = next.lines.map((line) => ({
			description: line.description,
			quantity: decimalInput(line.quantity),
			unit: line.unit,
			price: moneyInput(line.unit_price, next.currency),
			vat: decimalInput(line.vat_rate),
			project_id: line.project_id ?? null,
			activity_id: line.activity_id ?? null,
			time_entry_ids: line.time_entry_ids,
			cancels_line_id: line.cancels_line_id ?? null
		}));
		dirty = false;
		issueError = undefined;
	}

	$effect(() => {
		call(api.GET('/api/documents/{id}', { params: { path: { id } } })).then((result) => {
			if (result.data) show(result.data);
			error = result.error;
		});
	});

	$effect(() => {
		if (!owner || doc?.status !== 'draft') return;
		call(api.GET('/api/customers')).then(({ data }) => (customers = data ?? []));
		call(api.GET('/api/activities')).then(({ data }) => (activities = data ?? []));
		call(api.GET('/api/snippets')).then(({ data }) => (snippets = data ?? []));
	});

	$effect(() => {
		const customer_id = customerId;
		if (!customer_id || doc?.status !== 'draft') return;
		call(api.GET('/api/projects', { params: { query: { customer_id } } })).then(
			({ data }) => (projects = data ?? [])
		);
	});

	function body(): Schemas['DocumentInput'] | undefined {
		if (!doc) return;
		const parsed = lines.map((line) => ({
			description: line.description,
			quantity: parseDecimal(line.quantity),
			unit: line.unit,
			unit_price: parseMoney(line.price, doc!.currency),
			vat_rate: parseDecimal(line.vat),
			project_id: line.project_id,
			activity_id: line.activity_id,
			time_entry_ids: line.time_entry_ids,
			cancels_line_id: line.cancels_line_id
		}));
		const fields: Record<string, Schemas['FieldError']> = {};
		parsed.forEach((line, i) => {
			if (line.quantity == null || Number.isNaN(line.quantity)) fields[`lines.${i}.quantity`] = 'invalid_format';
			if (Number.isNaN(line.unit_price)) fields[`lines.${i}.unit_price`] = 'invalid_format';
			if (Number.isNaN(line.vat_rate)) fields[`lines.${i}.vat_rate`] = 'invalid_format';
		});
		if (Object.keys(fields).length) {
			error = { ...clientError('error.invalid_input'), fields };
			return;
		}
		return {
			kind: doc.kind,
			customer_id: customerId,
			project_id: projectId,
			language,
			title,
			intro,
			outro,
			valid_until: doc.kind === 'quote' ? validUntil || null : null,
			pricing: doc.kind === 'quote' ? pricing : null,
			service_from: doc.kind === 'invoice' ? serviceFrom || null : null,
			service_to: doc.kind === 'invoice' ? serviceTo || null : null,
			timesheet: doc.kind === 'invoice' && timesheet,
			lines: parsed as Schemas['LineInput'][]
		};
	}

	async function save(): Promise<boolean> {
		const input = body();
		if (!input) return false;
		busy = true;
		const result = await call(api.PUT('/api/documents/{id}', { params: { path: { id } }, body: input }));
		busy = false;
		error = result.error;
		if (result.data) show(result.data);
		return !result.error;
	}

	async function issue() {
		if (dirty && !(await save())) return;
		if (!confirm(t('documents.issue_confirm'))) return;
		busy = true;
		const result = await call(api.POST('/api/documents/{id}/issue', { params: { path: { id } } }));
		busy = false;
		issueError = result.error?.fields ? result.error : undefined;
		error = result.error?.fields ? undefined : result.error;
		if (result.data) {
			show(result.data);
			toast(t('documents.issued_done', { number: result.data.number ?? '' }));
		}
	}

	async function remove() {
		if (!confirm(t('documents.delete_confirm'))) return;
		const result = await call(api.DELETE('/api/documents/{id}', { params: { path: { id } } }));
		error = result.error;
		if (!result.error) goto('/documents');
	}

	async function duplicate() {
		const result = await call(api.POST('/api/documents/{id}/duplicate', { params: { path: { id } } }));
		error = result.error;
		if (result.data) goto(`/documents/${result.data.id}`);
	}

	async function followUp(kind: Kind) {
		const result = await call(
			api.POST('/api/documents/{id}/follow-up', { params: { path: { id } }, body: { kind } })
		);
		error = result.error;
		if (result.data) goto(`/documents/${result.data.id}`);
	}

	async function decide(outcome: 'accepted' | 'declined') {
		const result = await call(
			api.POST('/api/documents/{id}/outcome', { params: { path: { id } }, body: { outcome } })
		);
		error = result.error;
		if (result.data) show(result.data);
	}

	function addLine() {
		lines.push({
			description: '',
			quantity: '1',
			unit: 'hour',
			price: '',
			vat: '',
			project_id: projectId,
			activity_id: null,
			time_entry_ids: [],
			cancels_line_id: null
		});
		dirty = true;
	}

	let timeOpen = $state(false);
	let timeFrom = $state('');
	let timeTo = $state('');
	let timeProject = $state<number | null>(null);
	let grouping = $state<Schemas['Grouping']>('todo');
	let rounding = $state(15);
	let unbilled = $state<Schemas['UnbilledProject'][]>([]);
	let timeError = $state<ApiError>();

	let payDate = $state(new Date().toISOString().slice(0, 10));
	let payAmount = $state('');
	let payError = $state<ApiError>();

	async function pay(event: SubmitEvent) {
		event.preventDefault();
		if (!doc) return;
		const amount = parseMoney(payAmount, doc.currency);
		if (amount == null || Number.isNaN(amount)) {
			payError = { ...clientError('error.invalid_input'), fields: { amount: 'invalid_format' } };
			return;
		}
		const result = await call(
			api.POST('/api/documents/{id}/payments', { params: { path: { id } }, body: { date: payDate, amount } })
		);
		payError = result.error;
		if (result.data) {
			show(result.data);
			payAmount = '';
		}
	}

	async function unpay(paymentId: number) {
		if (!confirm(t('documents.payment_delete_confirm'))) return;
		const result = await call(api.DELETE('/api/payments/{id}', { params: { path: { id: paymentId } } }));
		error = result.error;
		const reload = await call(api.GET('/api/documents/{id}', { params: { path: { id } } }));
		if (reload.data) show(reload.data);
	}

	let sendOpen = $state(false);
	let sendTo = $state('');
	let sendSubject = $state('');
	let sendBody = $state('');
	let sendError = $state<ApiError>();

	function openSend() {
		sendTo = doc?.data.customer.email ?? '';
		sendSubject = '';
		sendBody = '';
		sendError = undefined;
		sendOpen = true;
		if (!snippets.length) call(api.GET('/api/snippets')).then(({ data }) => (snippets = data ?? []));
	}

	async function send(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		const result = await call(
			api.POST('/api/documents/{id}/send', {
				params: { path: { id } },
				body: { to: sendTo || null, subject: sendSubject || null, body: sendBody || null }
			})
		);
		busy = false;
		sendError = result.error;
		if (result.data) {
			show(result.data);
			sendOpen = false;
			toast(t('documents.sent_done', { to: sendTo }));
		} else {
			const reload = await call(api.GET('/api/documents/{id}', { params: { path: { id } } }));
			if (reload.data) doc = reload.data;
		}
	}

	async function openTime() {
		if (dirty && !(await save())) return;
		timeError = undefined;
		timeOpen = true;
		rounding = doc?.rounding || 15;
		const result = await call(
			api.GET('/api/unbilled', { params: { query: { customer_id: customerId } } })
		);
		unbilled = result.data ?? [];
	}

	async function addTime(event: SubmitEvent) {
		event.preventDefault();
		const result = await call(
			api.POST('/api/documents/{id}/time', {
				params: { path: { id } },
				body: {
					from: timeFrom || null,
					to: timeTo || null,
					project_id: timeProject,
					group: grouping,
					rounding
				}
			})
		);
		timeError = result.error;
		if (result.data) {
			show(result.data);
			timeOpen = false;
		}
	}

	function move(i: number, by: number) {
		const [line] = lines.splice(i, 1);
		lines.splice(i + by, 0, line);
		dirty = true;
	}

	function insertSnippet(target: 'intro' | 'outro', snippetId: string) {
		const snippet = snippets.find((s) => String(s.id) === snippetId);
		if (!snippet) return;
		if (target === 'intro') intro = intro ? `${intro}\n\n${snippet.body}` : snippet.body;
		else outro = outro ? `${outro}\n\n${snippet.body}` : snippet.body;
		dirty = true;
	}

	const followUps: Record<Kind, Kind[]> = {
		quote: ['order_confirmation', 'invoice', 'delivery_note', 'acceptance_report'],
		order_confirmation: ['invoice', 'delivery_note', 'acceptance_report'],
		invoice: ['cancellation', 'delivery_note', 'acceptance_report'],
		cancellation: [],
		delivery_note: [],
		acceptance_report: []
	};
	const draft = $derived(doc?.status === 'draft' && owner);
	const priced = $derived(doc?.data.totals != null || (doc ? !['delivery_note', 'acceptance_report'].includes(doc.kind) : false));
	const canFollow = $derived(
		owner && doc?.status === 'issued' && (doc.kind !== 'quote' || doc.quote_state === 'accepted')
	);
	const fieldError = (key: string) => error?.fields?.[key] as Schemas['FieldError'] | undefined;
	const missing = $derived(Object.keys(issueError?.fields ?? doc?.missing ?? {}));
	const unitLabel = (unit: Schemas['Unit']) => t(`documents.unit.${unit}`);
</script>

{#if doc}
	<div class="page-head">
		<div>
			<p class="muted eyebrow">
				<a href="/documents">{t('documents.title')}</a> · {kindLabel(doc.kind)}
			</p>
			<h1>
				{doc.number ?? t('term.draft')}
				{#if doc.quote_state}<small class="badge" class:accent={doc.quote_state === 'accepted'}>{t(`documents.state.${doc.quote_state}`)}</small>{/if}
			</h1>
			<p class="muted">
				{doc.customer_name}{#if doc.issue_date}&nbsp;· {formatDay(doc.issue_date, { dateStyle: 'medium' })}{/if}
				{#if doc.layout}&nbsp;· {t('term.layout')}: {doc.layout}{/if}
			</p>
		</div>
		<div class="actions">
			<a class="button ghost" href={`/api/documents/${doc.id}/pdf`} target="_blank" rel="noopener">
				<Icon name="pdf" size={18} />{t(doc.status === 'draft' ? 'documents.preview' : 'documents.pdf')}
			</a>
			{#if doc.kind === 'invoice' || doc.kind === 'cancellation'}
				<a class="button ghost" href={`/api/documents/${doc.id}/xml`}>{t('documents.xml')}</a>
			{/if}
			{#if owner}
				<button class="ghost" onclick={duplicate}>{t('documents.duplicate')}</button>
			{/if}
			{#if owner && doc.status === 'issued'}
				<button class="primary" onclick={openSend}><Icon name="send" size={18} />{t('documents.send')}</button>
			{/if}
			{#if draft}
				<button class="ghost" onclick={remove}>{t('app.delete')}</button>
				<button class="primary" onclick={issue} disabled={busy}>{t('documents.issue')}</button>
			{/if}
		</div>
	</div>

	<div class="stack">
		{#if error && error.code !== 'invalid_input' && !error.fields}
			<div class="alert" role="alert">
				{t(error.message)}
				{#if error.detail}<pre>{error.detail}</pre>{/if}
			</div>
		{/if}
		{#if draft && missing.length}
			<div class="card notice">
				<strong>{t(issueError ? 'documents.cannot_issue' : 'documents.before_issue')}</strong>
				<ul>
					{#each missing as key (key)}
						<li>{missingLabel(key)}{#if issueError?.fields?.[key] === 'not_shown'}&nbsp;– {t('field.not_shown')}{/if}</li>
					{/each}
				</ul>
			</div>
		{/if}

		{#if doc.source_id || doc.follow_ups.length}
			<div class="card links">
				{#if doc.source_id}
					<a href={`/documents/${doc.source_id}`}>{doc.data.reference ?? t('documents.source')}</a>
				{/if}
				{#each doc.follow_ups as follow (follow.id)}
					<a href={`/documents/${follow.id}`}>{kindLabel(follow.kind)} {follow.number ?? t('term.draft')}</a>
				{/each}
			</div>
		{/if}

		{#if owner && doc.kind === 'quote' && doc.status === 'issued' && (doc.quote_state === 'open' || doc.quote_state === 'expired')}
			<div class="card actions">
				<span>{t('documents.quote_answer')}</span>
				<button class="primary" onclick={() => decide('accepted')}><Icon name="check" size={18} />{t('documents.accept')}</button>
				<button class="ghost" onclick={() => decide('declined')}>{t('documents.decline')}</button>
			</div>
		{/if}
		{#if canFollow && followUps[doc.kind].length}
			<div class="card actions">
				<span>{t('documents.follow_up')}</span>
				{#each followUps[doc.kind] as kind (kind)}
					<button class="ghost" onclick={() => followUp(kind)}><Icon name="plus" size={18} />{kindLabel(kind)}</button>
				{/each}
			</div>
		{/if}

		{#if draft}
			<form
				class="card grid"
				oninput={() => (dirty = true)}
				onchange={() => (dirty = true)}
				onsubmit={(e) => {
					e.preventDefault();
					save();
				}}
			>
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
				<Field label={t('customers.language')}>
					<select bind:value={language}>
						<option value="de">{t('language.de')}</option>
						<option value="en">{t('language.en')}</option>
					</select>
				</Field>
				{#if doc.kind === 'invoice'}
					<Field label={t('documents.service_from')} error={fieldError('service_from')} note={t('documents.service_hint')}>
						<input type="date" bind:value={serviceFrom} />
					</Field>
					<Field label={t('documents.service_to')} error={fieldError('service_to')}>
						<input type="date" bind:value={serviceTo} />
					</Field>
					<div class="wide">
						<Field label={t('documents.timesheet')} inline>
							<input type="checkbox" bind:checked={timesheet} />
						</Field>
					</div>
				{/if}
				{#if doc.kind === 'quote'}
					<Field label={t('documents.valid_until')} error={fieldError('valid_until')} note={t('documents.valid_until_hint')}>
						<input type="date" bind:value={validUntil} />
					</Field>
					<div class="wide">
						<Segmented
							label={t('documents.pricing')}
							options={[
								{ value: 'fixed', label: t('term.fixed_price') },
								{ value: 'effort', label: t('term.by_effort') }
							]}
							value={pricing}
							onchange={(v) => {
								pricing = v;
								dirty = true;
							}}
						/>
					</div>
				{/if}
				<div class="wide">
					<Field label={t('documents.doc_title')} error={fieldError('title')}>
						<input bind:value={title} />
					</Field>
				</div>
				<div class="wide">
					<Field label={t('documents.intro')} error={fieldError('intro')}>
						<textarea rows="3" bind:value={intro}></textarea>
					</Field>
					{#if snippets.length}
						<select class="snippet" aria-label={t('documents.insert_snippet')} onchange={(e) => { insertSnippet('intro', e.currentTarget.value); e.currentTarget.value = ''; }}>
							<option value="">{t('documents.insert_snippet')}</option>
							{#each snippets.filter((s) => s.language === language) as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
						</select>
					{/if}
				</div>

				<div class="wide lines">
					<h2>{t('documents.lines')}</h2>
					{#if doc.kind === 'cancellation'}
						<p class="muted">{t('documents.cancellation_hint')}</p>
						{#each lines as line, i (line.cancels_line_id)}
							<div class="reversal">
								<span>{line.description}</span>
								<span class="muted">{line.quantity} {unitLabel(line.unit)}</span>
								<button type="button" class="ghost icon" aria-label={t('app.remove')} disabled={lines.length === 1} onclick={() => { lines.splice(i, 1); dirty = true; }}><Icon name="close" size={18} /></button>
							</div>
						{/each}
					{:else}
					{#each lines as line, i (i)}
						<div class="line">
							<div class="description">
								<Field label={t('documents.description')} error={fieldError(`lines.${i}.description`)}>
									<textarea rows="2" bind:value={line.description}></textarea>
								</Field>
							</div>
							<Field label={t('documents.quantity')} error={fieldError(`lines.${i}.quantity`)}>
								<input inputmode="decimal" bind:value={line.quantity} />
							</Field>
							<Field label={t('documents.unit')}>
								<select bind:value={line.unit}>
									{#each units as unit (unit)}<option value={unit}>{unitLabel(unit)}</option>{/each}
								</select>
							</Field>
							{#if priced}
								<Field
									label={t('documents.unit_price')}
									error={fieldError(`lines.${i}.unit_price`)}
									note={!line.price && line.project_id ? t('documents.rate_hint') : doc.currency}
								>
									<input inputmode="decimal" bind:value={line.price} />
								</Field>
								<Field label={t('documents.vat')} error={fieldError(`lines.${i}.vat_rate`)}>
									<input inputmode="decimal" bind:value={line.vat} placeholder={doc.data.business.small_business ? '0' : '19'} />
								</Field>
								<Field label={t('term.project')}>
									<select bind:value={line.project_id}>
										<option value={null}>–</option>
										{#each projects as project (project.id)}<option value={project.id}>{project.name}</option>{/each}
									</select>
								</Field>
								<Field label={t('term.activity')}>
									<select bind:value={line.activity_id}>
										<option value={null}>–</option>
										{#each activities as activity (activity.id)}<option value={activity.id}>{activity.name}</option>{/each}
									</select>
								</Field>
							{/if}
							{#if line.time_entry_ids.length}
								<small class="muted wide-line">{t('documents.from_time', { n: line.time_entry_ids.length })}</small>
							{/if}
							<div class="line-actions">
								<button type="button" class="ghost icon" aria-label={t('documents.up')} disabled={i === 0} onclick={() => move(i, -1)}><Icon name="chevron_up" size={18} /></button>
								<button type="button" class="ghost icon" aria-label={t('documents.down')} disabled={i === lines.length - 1} onclick={() => move(i, 1)}><Icon name="chevron_down" size={18} /></button>
								<button type="button" class="ghost icon" aria-label={t('app.remove')} onclick={() => { lines.splice(i, 1); dirty = true; }}><Icon name="close" size={18} /></button>
							</div>
						</div>
					{/each}
					{/if}
					{#if doc.kind !== 'cancellation'}
					<div class="add">
						<button type="button" class="ghost" onclick={addLine}><Icon name="plus" size={18} />{t('documents.add_line')}</button>
						{#if doc.kind === 'invoice'}
							<button type="button" class="ghost" onclick={openTime}><Icon name="time" size={18} />{t('documents.add_time')}</button>
						{/if}
					</div>
					{/if}
				</div>

				<div class="wide">
					<Field label={t('documents.outro')} error={fieldError('outro')}>
						<textarea rows="3" bind:value={outro}></textarea>
					</Field>
					{#if snippets.length}
						<select class="snippet" aria-label={t('documents.insert_snippet')} onchange={(e) => { insertSnippet('outro', e.currentTarget.value); e.currentTarget.value = ''; }}>
							<option value="">{t('documents.insert_snippet')}</option>
							{#each snippets.filter((s) => s.language === language) as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
						</select>
					{/if}
				</div>
				<div class="wide save">
					{#if doc.data.totals}
						<span class="muted">{t('documents.total')}: <strong>{formatAmount(doc.data.totals.gross, doc.currency)}</strong>{#if dirty}&nbsp;({t('documents.unsaved')}){/if}</span>
					{/if}
					<button class="primary" disabled={busy || !dirty}>{t('app.save')}</button>
				</div>
			</form>
		{:else}
			<section class="card">
				{#if doc.data.title}<h2>{doc.data.title}</h2>{/if}
				{#if doc.data.intro}<p class="pre">{doc.data.intro}</p>{/if}
				<div class="table">
					<table>
						<thead>
							<tr>
								<th>{t('documents.description')}</th>
								<th class="num">{t('documents.quantity')}</th>
								{#if doc.data.totals}<th class="num wide-only">{t('documents.unit_price')}</th><th class="num">{t('documents.amount')}</th>{/if}
							</tr>
						</thead>
						<tbody>
							{#each doc.data.lines as line (line.position)}
								<tr>
									<td class="pre">{line.description}</td>
									<td class="num">{decimalInput(line.quantity)} {unitLabel(line.unit)}</td>
									{#if doc.data.totals}
										<td class="num wide-only">{formatAmount(line.unit_price, doc.currency)}</td>
										<td class="num">{formatAmount(line.net, doc.currency)}</td>
									{/if}
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
				{#if doc.data.totals}
					<dl class="totals">
						<dt>{t('documents.net')}</dt><dd>{formatAmount(doc.data.totals.net, doc.currency)}</dd>
						{#each doc.data.vat.filter((v) => Number(v.rate) > 0) as group (group.rate)}
							<dt>{t('documents.vat_at', { rate: decimalInput(group.rate) })}</dt><dd>{formatAmount(group.tax, doc.currency)}</dd>
						{/each}
						<dt><strong>{t('documents.total')}</strong></dt><dd><strong>{formatAmount(doc.data.totals.gross, doc.currency)}</strong></dd>
					</dl>
				{/if}
				{#if doc.data.outro}<p class="pre">{doc.data.outro}</p>{/if}
				{#if doc.data.valid_until}<p class="muted">{t('documents.valid_until')}: {formatDay(doc.data.valid_until, { dateStyle: 'medium' })}</p>{/if}
				{#if doc.data.service_from && doc.data.service_to}<p class="muted">{t('term.service_period')}: {formatDay(doc.data.service_from, { dateStyle: 'medium' })} – {formatDay(doc.data.service_to, { dateStyle: 'medium' })}</p>{/if}
				{#if doc.data.due_date}<p class="muted">{t('documents.due_date')}: {formatDay(doc.data.due_date, { dateStyle: 'medium' })}</p>{/if}
			</section>
		{/if}
		{#if doc.amount_due}
			<section class="card stack">
				<h2>{t('documents.payments')}</h2>
				<p>
					{t('documents.amount_due')}:
					<strong>{formatMoney(doc.amount_due.minor, doc.amount_due.currency)}</strong>
					{#if doc.amount_due.minor <= 0}<small class="badge accent">{t('documents.paid')}</small>{/if}
				</p>
				{#if doc.payments.length}
					<ul class="sent">
						{#each doc.payments as payment (payment.id)}
							<li>
								<span>{formatDay(payment.date, { dateStyle: 'medium' })}</span>
								<span>{formatMoney(payment.amount, doc.currency)}</span>
								<span class="muted">{payment.user_name}</span>
								{#if owner}<button class="ghost icon" aria-label={t('app.remove')} onclick={() => unpay(payment.id)}><Icon name="close" size={18} /></button>{/if}
							</li>
						{/each}
					</ul>
				{/if}
				{#if owner && doc.amount_due.minor > 0}
					<form class="pay" onsubmit={pay}>
						<Field label={t('documents.payment_date')} error={payError?.fields?.date}>
							<input type="date" bind:value={payDate} />
						</Field>
						<Field label={t('documents.payment_amount')} error={payError?.fields?.amount} note={doc.currency}>
							<input inputmode="decimal" bind:value={payAmount} placeholder={moneyInput(doc.amount_due.minor, doc.currency)} />
						</Field>
						<button class="primary">{t('documents.record_payment')}</button>
					</form>
				{/if}
			</section>
		{/if}
		{#if doc.sent.length}
			<section class="card">
				<h2>{t('documents.sent_log')}</h2>
				<ul class="sent">
					{#each doc.sent as entry, i (i)}
						<li>
							<span>{new Date(entry.at * 1000).toLocaleString()}</span>
							<span>{entry.recipient}</span>
							<span class="muted">{entry.user_name}</span>
							{#if entry.error}<span class="error">{t('documents.send_failed')}: {entry.error}</span>{:else}<span class="badge accent">{t('documents.sent_ok')}</span>{/if}
						</li>
					{/each}
				</ul>
			</section>
		{/if}
	</div>
{:else if error}
	<p class="alert" role="alert">{t(error.message)}</p>
{/if}


<Dialog bind:open={timeOpen} title={t('documents.add_time')}>
	<form class="stack" onsubmit={addTime}>
		{#if unbilled.length}
			<ul class="unbilled">
				{#each unbilled as project (project.project_id)}
					<li>
						<span>{project.project_name}</span>
						<span class="muted">{t('documents.unbilled_hours', { hours: (project.minutes / 60).toLocaleString(undefined, { maximumFractionDigits: 2 }) })}</span>
					</li>
				{/each}
			</ul>
		{:else}
			<p class="muted">{t('documents.no_unbilled')}</p>
		{/if}
		<div class="pair">
			<Field label={t('documents.time_from')} error={timeError?.fields?.from}>
				<input type="date" bind:value={timeFrom} />
			</Field>
			<Field label={t('documents.time_to')} error={timeError?.fields?.to}>
				<input type="date" bind:value={timeTo} />
			</Field>
		</div>
		<Field label={t('term.project')}>
			<select bind:value={timeProject}>
				<option value={null}>{t('documents.all_projects')}</option>
				{#each unbilled as project (project.project_id)}<option value={project.project_id}>{project.project_name}</option>{/each}
			</select>
		</Field>
		<Field label={t('documents.grouping')}>
			<select bind:value={grouping}>
				<option value="todo">{t('documents.group.todo')}</option>
				<option value="project">{t('documents.group.project')}</option>
				<option value="activity">{t('documents.group.activity')}</option>
				<option value="entry">{t('documents.group.entry')}</option>
			</select>
		</Field>
		<Field label={t('documents.rounding')} error={timeError?.fields?.rounding}>
			<select bind:value={rounding}>
				{#each [0, 5, 10, 15, 30] as minutes (minutes)}
					<option value={minutes}>{minutes ? t('documents.rounding_to', { minutes }) : t('documents.rounding_none')}</option>
				{/each}
			</select>
		</Field>
		{#if timeError && !timeError.fields}<p class="alert" role="alert">{t(timeError.message)}</p>{/if}
		<div><button class="primary">{t('documents.add_time')}</button></div>
	</form>
</Dialog>


<Dialog bind:open={sendOpen} title={t('documents.send')}>
	<form class="stack" onsubmit={send}>
		<Field label={t('documents.send_to')} error={sendError?.fields?.to}>
			<input type="email" bind:value={sendTo} />
		</Field>
		<Field label={t('documents.send_subject')} error={sendError?.fields?.subject} note={t('documents.send_default')}>
			<input bind:value={sendSubject} />
		</Field>
		<Field label={t('documents.send_body')} error={sendError?.fields?.body} note={t('documents.send_default')}>
			<textarea rows="6" bind:value={sendBody}></textarea>
		</Field>
		{#if snippets.length}
			<select class="snippet" aria-label={t('documents.insert_snippet')} onchange={(e) => { const s = snippets.find((x) => String(x.id) === e.currentTarget.value); if (s) sendBody = sendBody ? `${sendBody}\n\n${s.body}` : s.body; e.currentTarget.value = ''; }}>
				<option value="">{t('documents.insert_snippet')}</option>
				{#each snippets.filter((s) => s.language === doc?.language) as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
			</select>
		{/if}
		{#if sendError && !sendError.fields}
			<div class="alert" role="alert">{t(sendError.message)}{#if sendError.detail}<pre>{sendError.detail}</pre>{/if}</div>
		{/if}
		<div><button class="primary" disabled={busy}>{t('documents.send')}</button></div>
	</form>
</Dialog>

<style>
	.eyebrow {
		margin: 0;
		font-size: 0.875rem;
	}
	h1 {
		display: flex;
		gap: 0.5rem;
		align-items: center;
		flex-wrap: wrap;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem 0.75rem;
		align-items: center;
	}
	.links {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem 1.25rem;
	}
	.notice ul {
		margin: 0.5rem 0 0;
		padding-left: 1.25rem;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr));
		gap: 1rem 1.25rem;
		max-width: 64rem;
	}
	.wide {
		grid-column: 1 / -1;
	}
	h2 {
		margin: 0 0 0.75rem;
		font-size: 1.125rem;
	}
	.lines {
		display: grid;
		gap: 1rem;
	}
	.line {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(8rem, 1fr));
		gap: 0.75rem;
		padding: 0.875rem;
		border: 1px solid var(--border);
		border-radius: var(--radius, 0.5rem);
		align-items: start;
	}
	.description {
		grid-column: 1 / -1;
	}
	.line-actions {
		display: flex;
		gap: 0.25rem;
		align-self: end;
		justify-content: end;
	}
	.snippet {
		margin-top: 0.375rem;
		width: auto;
		max-width: 100%;
	}
	.save {
		display: flex;
		flex-wrap: wrap;
		justify-content: space-between;
		align-items: center;
		gap: 0.75rem;
	}
	section {
		max-width: 64rem;
	}
	.table {
		overflow-x: auto;
	}
	table {
		width: 100%;
		border-collapse: collapse;
	}
	th,
	td {
		padding: 0.5rem;
		border-bottom: 1px solid var(--border);
		text-align: left;
		vertical-align: top;
	}
	.num {
		text-align: right;
		white-space: nowrap;
		font-variant-numeric: tabular-nums;
	}
	.totals {
		display: grid;
		grid-template-columns: auto auto;
		justify-content: end;
		gap: 0.375rem 1.5rem;
		margin: 1rem 0;
	}
	.totals dd {
		margin: 0;
		text-align: right;
		font-variant-numeric: tabular-nums;
	}
	pre {
		white-space: pre-wrap;
		font-size: 0.75rem;
		margin: 0.5rem 0 0;
	}
	.reversal {
		display: grid;
		grid-template-columns: 1fr auto auto;
		gap: 0.75rem;
		align-items: center;
		padding: 0.5rem 0;
		border-bottom: 1px solid var(--border);
	}
	.pay {
		display: flex;
		flex-wrap: wrap;
		gap: 0.75rem;
		align-items: end;
	}
	.sent {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: 0.5rem;
	}
	.sent li {
		display: flex;
		flex-wrap: wrap;
		gap: 0.25rem 1rem;
		align-items: center;
	}
	.error {
		color: var(--danger);
	}
	.add {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}
	.wide-line {
		grid-column: 1 / -1;
	}
	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 0.75rem;
	}
	.unbilled {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: 0.375rem;
	}
	.unbilled li {
		display: flex;
		justify-content: space-between;
		gap: 1rem;
	}
	@media (max-width: 40rem) {
		.wide-only {
			display: none;
		}
	}
</style>
