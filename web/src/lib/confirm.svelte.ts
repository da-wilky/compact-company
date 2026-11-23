export type Question = {
	text: string;
	/** The confirm button's label, such as "Löschen"; also the title unless one is given. */
	action: string;
	title?: string;
	/** Destructive: styled as such, and focus starts on cancel. */
	danger?: boolean;
	/** Asks for a value instead of a yes; `check` returns a message for an invalid one. */
	input?: { label: string; inputmode?: 'decimal' | 'text'; check?: (value: string) => string | undefined };
};

export const asking = $state<{ question?: Question; answer?: (value: string | null) => void }>({});

function ask(question: Question): Promise<string | null> {
	asking.answer?.(null);
	return new Promise((resolve) => {
		asking.question = question;
		asking.answer = resolve;
	});
}

/** The app's own `confirm()`: resolves true on the confirm button, false otherwise. */
export async function confirm(question: Omit<Question, 'input'>): Promise<boolean> {
	return (await ask(question)) !== null;
}

/** The app's own `prompt()`: resolves the entered value, or null when cancelled. */
export function prompt(question: Question & { input: NonNullable<Question['input']> }): Promise<string | null> {
	return ask(question);
}
