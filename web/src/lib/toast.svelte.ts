export const toasts = $state<{ id: number; text: string }[]>([]);

let next = 0;

export function toast(text: string) {
	const id = next++;
	toasts.push({ id, text });
	setTimeout(() => toasts.splice(toasts.findIndex((t) => t.id === id), 1), 3000);
}
