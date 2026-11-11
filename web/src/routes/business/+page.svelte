<script lang="ts">
	import { api, call, type ApiError, type Schemas } from '#lib/api/index.ts';
	import Field from '#lib/components/Field.svelte';
	import { t } from '#lib/i18n.svelte.ts';
	import { session } from '#lib/session.svelte.ts';
	import { toast } from '#lib/toast.svelte.ts';

	type Business = Schemas['Business'];
	type TextKey = {
		[K in keyof Business]-?: NonNullable<Business[K]> extends string ? K : never;
	}[keyof Business];

	const owner = $derived(session.view?.user?.role === 'owner');

	let values = $state<Business>({});
	let pinned = $state<string[]>([]);
	let error = $state<ApiError>();

	function show(result: { data?: Schemas['BusinessView']; error?: ApiError }) {
		if (result.data) {
			values = { ...result.data.values, taxation: result.data.values.taxation ?? 'accrual' };
			pinned = result.data.pinned;
		}
		error = result.error;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const body = Object.fromEntries(
			Object.entries(values).map(([key, value]) => [key, value === '' ? null : value])
		) as Business;
		const result = await call(api.PUT('/api/business', { body }));
		show(result);
		if (!result.error) toast(t('app.saved'));
	}

	call(api.GET('/api/business')).then(show);
	let currencies = $state<string[]>([]);
	call(api.GET('/api/currencies')).then(({ data }) => (currencies = data ?? []));

	const note = (key: string) => (pinned.includes(key) ? t('business.pinned') : undefined);
	const sections: { title: string; keys: TextKey[] }[] = [
		{
			title: 'business.section_company',
			keys: ['name', 'street', 'postcode', 'city', 'country', 'email', 'phone', 'website']
		},
		{
			title: 'business.section_legal',
			keys: [
				'tax_number',
				'vat_id',
				'legal_form',
				'register_court',
				'register_number',
				'managing_directors'
			]
		},
		{ title: 'business.section_bank', keys: ['bank_name', 'iban', 'bic'] },
		{
			title: 'business.section_numbers',
			keys: [
				'quote_number_format',
				'order_confirmation_number_format',
				'invoice_number_format',
				'delivery_note_number_format',
				'acceptance_report_number_format',
				'dunning_number_format'
			]
		}
	];
	const placeholders: Partial<Record<TextKey, string>> = {
		country: 'DE',
		quote_number_format: 'AN-{yyyy}-{nnn}',
		order_confirmation_number_format: 'AB-{yyyy}-{nnn}',
		invoice_number_format: 'RE-{yyyy}-{nnn}',
		delivery_note_number_format: 'LS-{yyyy}-{nnn}',
		acceptance_report_number_format: 'AP-{yyyy}-{nnn}',
		dunning_number_format: 'MA-{yyyy}-{nnn}',
		legal_form: 'UG (haftungsbeschränkt)'
	};
</script>

<div class="page-head">
	<h1>{t('business.title')}</h1>
</div>

<form onsubmit={save}>
	<fieldset disabled={!owner}>
		{#each sections as section (section.title)}
			<section class="card">
				<h2>{t(section.title)}</h2>
				{#if section.title === 'business.section_numbers'}
					<p class="muted wide">{t('business.numbers_hint')}</p>
				{/if}
				{#each section.keys as key (key)}
					<Field label={t(`business.${key}`)} error={error?.fields?.[key]} note={note(key)}>
						<input
							bind:value={values[key]}
							readonly={pinned.includes(key)}
							placeholder={placeholders[key]}
						/>
					</Field>
				{/each}
				{#if section.title === 'business.section_legal'}
					<Field label={t('term.taxation')} note={note('taxation')}>
						<select bind:value={values.taxation} disabled={pinned.includes('taxation')}>
							{#each ['accrual', 'cash', 'small_business'] as const as taxation (taxation)}
								<option value={taxation}>{t(`business.taxation.${taxation}`)}</option>
							{/each}
						</select>
					</Field>
				{/if}
			</section>
		{/each}
		<section class="card">
			<h2>{t('business.section_defaults')}</h2>
			<Field label={t('business.currency')} error={error?.fields?.currency} note={note('currency')}>
				<select bind:value={values.currency} disabled={pinned.includes('currency')}>
					{#each currencies as code (code)}<option value={code}>{code}</option>{/each}
				</select>
			</Field>
			<Field label={t('business.vat_rate')} error={error?.fields?.vat_rate} note={note('vat_rate')}>
				<input
					inputmode="decimal"
					bind:value={values.vat_rate}
					readonly={pinned.includes('vat_rate')}
					placeholder="19"
				/>
			</Field>
			<Field
				label={t('business.payment_days')}
				error={error?.fields?.payment_days}
				note={note('payment_days')}
			>
				<input
					type="number"
					min="0"
					max="365"
					bind:value={values.payment_days}
					readonly={pinned.includes('payment_days')}
					placeholder="14"
				/>
			</Field>
			<Field
				label={t('business.quote_valid_days')}
				error={error?.fields?.quote_valid_days}
				note={note('quote_valid_days')}
			>
				<input
					type="number"
					min="1"
					max="365"
					bind:value={values.quote_valid_days}
					readonly={pinned.includes('quote_valid_days')}
					placeholder="30"
				/>
			</Field>
			<Field label={t('business.timezone')} error={error?.fields?.timezone} note={note('timezone')}>
				<input
					bind:value={values.timezone}
					readonly={pinned.includes('timezone')}
					placeholder="Europe/Berlin"
				/>
			</Field>
		</section>
		<section class="card">
			<h2>{t('business.section_tax')}</h2>
			{#each ['datev_consultant_number', 'datev_client_number'] as const as key (key)}
				<Field label={t(`business.${key}`)} error={error?.fields?.[key]} note={note(key)}>
					<input
						type="number"
						bind:value={values[key]}
						readonly={pinned.includes(key)}
						placeholder={key === 'datev_consultant_number' ? '1001' : '1'}
					/>
				</Field>
			{/each}
		</section>

		{#if error}<p class="alert" role="alert">{t(error.message)}</p>{/if}
		{#if owner}<div><button class="primary">{t('app.save')}</button></div>{/if}
	</fieldset>
</form>

<style>
	form {
		max-width: 48rem;
	}
	fieldset {
		display: grid;
		gap: 1.25rem;
		border: 0;
		padding: 0;
		margin: 0;
	}
	section {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr));
		gap: 1.25rem;
	}
	h2,
	.wide {
		grid-column: 1 / -1;
		margin: 0;
	}
</style>
