import { api, call, type ApiError, type Schemas } from './api/index.ts';

/** The signed-in user's running timers, the ticking clock they count against, and the start dialog. */
export const timers = $state({
	list: [] as Schemas['TimeEntry'][],
	now: Math.floor(Date.now() / 1000),
	dialog: false,
	preset: {} as { project_id?: number; todo_id?: number },
	error: undefined as ApiError | undefined
});

setInterval(() => {
	if (timers.list.length) timers.now = Math.floor(Date.now() / 1000);
}, 1000);

export async function loadTimers() {
	const result = await call(api.GET('/api/timers'));
	timers.list = result.data ?? [];
	timers.now = Math.floor(Date.now() / 1000);
}

export function openTimer(preset: { project_id?: number; todo_id?: number } = {}) {
	timers.preset = preset;
	timers.dialog = true;
}

export async function stopTimer(id: number) {
	const result = await call(api.POST('/api/time/{id}/stop', { params: { path: { id } } }));
	timers.error = result.error;
	await loadTimers();
	return result.data;
}

/** What a time entry is about, in the fewest words. */
export const entryTitle = (entry: Schemas['TimeEntry']) =>
	entry.todo_title ?? entry.description ?? entry.project_name;

/** 1:05:09 for a running timer, 1:05 for a finished duration. */
export function clock(secs: number, seconds = false): string {
	const h = Math.floor(secs / 3600);
	const m = Math.floor((secs % 3600) / 60);
	const s = secs % 60;
	const pad = (n: number) => String(n).padStart(2, '0');
	return seconds ? `${h}:${pad(m)}:${pad(s)}` : `${h}:${pad(m)}`;
}
