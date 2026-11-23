<script lang="ts">
	import { untrack } from 'svelte';
	import { asking } from '#lib/confirm.svelte.ts';
	import { t } from '#lib/i18n.svelte.ts';
	import Dialog from './Dialog.svelte';
	import Field from './Field.svelte';

	let open = $state(false);
	let value = $state('');
	let invalid = $state<string>();
	let cancelButton = $state<HTMLButtonElement>();
	let confirmButton = $state<HTMLButtonElement>();
	let field = $state<HTMLInputElement>();

	$effect(() => {
		if (!asking.question) return;
		value = '';
		invalid = undefined;
		open = true;
		const question = asking.question;
		// after the dialog's showModal, which focuses its first control
		setTimeout(() => (question.input ? field : question.danger ? cancelButton : confirmButton)?.focus());
	});

	$effect(() => {
		if (!open) untrack(() => finish(null));
	});

	function finish(answer: string | null) {
		const done = asking.answer;
		asking.question = undefined;
		asking.answer = undefined;
		open = false;
		done?.(answer);
	}

	function submit(event: SubmitEvent) {
		event.preventDefault();
		const check = asking.question?.input?.check;
		invalid = check?.(value);
		if (!invalid) finish(asking.question?.input ? value : '');
	}
</script>

<Dialog bind:open title={asking.question?.title ?? asking.question?.action ?? ''}>
	{#if asking.question}
		<form class="stack" onsubmit={submit}>
			<p>{asking.question.text}</p>
			{#if asking.question.input}
				<Field label={asking.question.input.label}>
					<input bind:this={field} bind:value inputmode={asking.question.input.inputmode ?? 'text'} class:invalid />
				</Field>
				{#if invalid}<small class="error" role="alert">{invalid}</small>{/if}
			{/if}
			<div class="row">
				<button type="button" class="ghost" bind:this={cancelButton} onclick={() => finish(null)}>{t('app.cancel')}</button>
				<button class={asking.question.danger ? 'danger' : 'primary'} bind:this={confirmButton}>{asking.question.action}</button>
			</div>
		</form>
	{/if}
</Dialog>

<style>
	p {
		margin: 0;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		justify-content: flex-end;
		gap: 0.5rem;
	}
	.error {
		color: var(--danger);
		font-size: 0.75rem;
	}
	.invalid {
		border-color: var(--danger);
	}
</style>
