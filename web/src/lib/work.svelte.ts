import { api, call, type Schemas } from './api/index.ts';

/** What todo forms and filters choose from, loaded once per page. */
export async function loadWork() {
	const [statuses, projects, assignees] = await Promise.all([
		call(api.GET('/api/statuses')),
		call(api.GET('/api/projects', { params: { query: { archived: false } } })),
		call(api.GET('/api/assignees'))
	]);
	return {
		statuses: statuses.data ?? ([] as Schemas['Status'][]),
		// archived customers keep their projects unarchived, but closed to new work
		projects: (projects.data ?? []).filter((p) => !p.closed),
		assignees: assignees.data ?? ([] as Schemas['Assignee'][])
	};
}
