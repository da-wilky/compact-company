// Screenshots every screen at desktop and phone width in light and dark, and fails on horizontal
// overflow and browser errors. Usage: ui-check.mjs <server> <chromium> [out-dir]
import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const [serverBin, chromiumBin, outArg = 'ui-check'] = process.argv.slice(2);
if (!serverBin || !chromiumBin) {
	console.error('usage: ui-check.mjs <server> <chromium> [out-dir]');
	process.exit(2);
}
const out = resolve(outArg);
mkdirSync(out, { recursive: true });

const widths = { desktop: { width: 1280, height: 800, mobile: false }, phone: { width: 390, height: 844, mobile: true } };
const themes = ['light', 'dark'];
const failures = [];
const work = mkdtempSync(join(tmpdir(), 'ui-check-'));
const children = [];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function freePort() {
	return new Promise((done) => {
		const probe = createServer().listen(0, '127.0.0.1', () => {
			const { port } = probe.address();
			probe.close(() => done(port));
		});
	});
}

function start(bin, args, onLine) {
	const child = spawn(bin, args, { stdio: ['ignore', 'pipe', 'pipe'] });
	children.push(child);
	for (const stream of [child.stdout, child.stderr]) {
		let rest = '';
		stream.on('data', (chunk) => {
			const lines = (rest + chunk).split('\n');
			rest = lines.pop();
			lines.forEach(onLine);
		});
	}
	return child;
}

async function until(what, check, timeout = 15000) {
	const end = Date.now() + timeout;
	while (Date.now() < end) {
		const value = await check();
		if (value) return value;
		await sleep(100);
	}
	throw new Error(`timed out waiting for ${what}`);
}

class Cdp {
	#ws;
	#next = 1;
	#pending = new Map();
	#listeners = new Map();

	static async connect(url) {
		const cdp = new Cdp();
		cdp.#ws = new WebSocket(url);
		await new Promise((ok, fail) => {
			cdp.#ws.onopen = ok;
			cdp.#ws.onerror = fail;
		});
		cdp.#ws.onmessage = ({ data }) => {
			const msg = JSON.parse(data);
			if (msg.id) {
				const { ok, fail } = cdp.#pending.get(msg.id);
				cdp.#pending.delete(msg.id);
				msg.error ? fail(new Error(msg.error.message)) : ok(msg.result);
			} else {
				(cdp.#listeners.get(msg.method) ?? []).forEach((f) => f(msg.params));
			}
		};
		return cdp;
	}

	send(method, params = {}) {
		const id = this.#next++;
		this.#ws.send(JSON.stringify({ id, method, params }));
		return new Promise((ok, fail) => this.#pending.set(id, { ok, fail }));
	}

	on(method, f) {
		this.#listeners.set(method, [...(this.#listeners.get(method) ?? []), f]);
	}

	close() {
		this.#ws.close();
	}
}

try {
	const port = await freePort();
	const origin = `http://localhost:${port}`;
	writeFileSync(
		join(work, 'config.toml'),
		`listen = "127.0.0.1:${port}"\npublic_url = "${origin}"\ndata_dir = "${join(work, 'data')}"\n`
	);
	let setupLink;
	start(serverBin, ['serve', '--config', join(work, 'config.toml')], (line) => {
		const found = line.match(/(http\S+setup#\S+)/);
		if (found) setupLink = found[1];
	});
	await until('the setup link', () => setupLink);

	const debugPort = await freePort();
	start(
		chromiumBin,
		[
			'--headless=new',
			`--remote-debugging-port=${debugPort}`,
			`--user-data-dir=${join(work, 'profile')}`,
			'--no-first-run',
			'--no-default-browser-check',
			'about:blank'
		],
		() => {}
	);
	const target = await until('Chromium', async () => {
		try {
			const pages = await (await fetch(`http://127.0.0.1:${debugPort}/json/list`)).json();
			return pages.find((p) => p.type === 'page');
		} catch {
			return undefined;
		}
	});
	const cdp = await Cdp.connect(target.webSocketDebuggerUrl);

	let errors = [];
	cdp.on('Runtime.exceptionThrown', ({ exceptionDetails }) =>
		errors.push(exceptionDetails.exception?.description ?? exceptionDetails.text)
	);
	cdp.on('Runtime.consoleAPICalled', ({ type, args }) => {
		if (type === 'error') errors.push(args.map((a) => a.value ?? a.description).join(' '));
	});
	cdp.on('Log.entryAdded', ({ entry }) => {
		if (entry.level === 'error') errors.push(`${entry.text} ${entry.url ?? ''}`.trim());
	});
	await cdp.send('Page.enable');
	await cdp.send('Runtime.enable');
	await cdp.send('Log.enable');
	await cdp.send('WebAuthn.enable');
	await cdp.send('WebAuthn.addVirtualAuthenticator', {
		options: {
			protocol: 'ctap2',
			transport: 'internal',
			hasResidentKey: true,
			hasUserVerification: true,
			isUserVerified: true,
			automaticPresenceSimulation: true
		}
	});

	const evaluate = async (expression) => {
		const { result, exceptionDetails } = await cdp.send('Runtime.evaluate', {
			expression,
			awaitPromise: true,
			returnByValue: true
		});
		if (exceptionDetails) throw new Error(`${expression}: ${exceptionDetails.text}`);
		return result.value;
	};
	const settle = () =>
		evaluate('document.fonts.ready.then(() => new Promise((r) => setTimeout(r, 500)))');

	async function view(width, theme) {
		const { width: w, height, mobile } = widths[width];
		await cdp.send('Emulation.setDeviceMetricsOverride', { width: w, height, deviceScaleFactor: 1, mobile });
		await cdp.send('Emulation.setTouchEmulationEnabled', { enabled: mobile });
		await cdp.send('Emulation.setEmulatedMedia', {
			features: [{ name: 'prefers-color-scheme', value: theme }]
		});
	}

	async function open(url) {
		await cdp.send('Page.navigate', { url: 'about:blank' });
		const loaded = new Promise((done) => cdp.on('Page.loadEventFired', done));
		await cdp.send('Page.navigate', { url });
		await loaded;
		await until(`${url} to render`, () => evaluate("!!document.querySelector('main')"));
		await settle();
	}

	async function shoot(name, width, theme) {
		const overflow = await evaluate('document.documentElement.scrollWidth - innerWidth');
		if (overflow > 0) failures.push(`${name} ${width} ${theme}: ${overflow}px horizontal overflow`);
		const { cssContentSize } = await cdp.send('Page.getLayoutMetrics');
		const { data } = await cdp.send('Page.captureScreenshot', {
			format: 'png',
			captureBeyondViewport: true,
			clip: { x: 0, y: 0, width: widths[width].width, height: Math.ceil(cssContentSize.height), scale: 1 }
		});
		writeFileSync(join(out, `${name}-${width}-${theme}.png`), Buffer.from(data, 'base64'));
	}

	// every screen at every size and theme; `prepare` brings the page into the state to show
	async function screen(name, url, prepare = async () => {}, only = Object.keys(widths)) {
		for (const width of only) {
			for (const theme of themes) {
				await view(width, theme);
				errors = [];
				await open(url);
				await prepare();
				await settle();
				await shoot(name, width, theme);
				for (const error of errors) failures.push(`${name} ${width} ${theme}: ${error}`);
			}
		}
	}

	const fill = (pairs) =>
		evaluate(`(() => {
			for (const [selector, value] of ${JSON.stringify(pairs)}) {
				const el = document.querySelector(selector);
				el.value = value;
				el.dispatchEvent(new Event('input', { bubbles: true }));
			}
		})()`);
	const click = (selector) => evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);

	await screen('setup', setupLink);

	await view('desktop', 'light');
	await open(setupLink);
	await fill([
		['form input:not([type])', 'Samuel Wilk'],
		['form label:nth-of-type(2) input', 'Laptop']
	]);
	await click('form button.primary');
	await until('the owner to be signed in', () => evaluate("location.pathname === '/' && !!document.querySelector('.shell')"));

	await open(`${origin}/business`);
	await fill([
		['form section:first-of-type label:nth-of-type(1) input', 'Beispiel GmbH'],
		['form section:first-of-type label:nth-of-type(2) input', 'Hauptstraße 1'],
		['form section:first-of-type label:nth-of-type(3) input', '12345'],
		['form section:first-of-type label:nth-of-type(4) input', 'Musterstadt'],
		['form section:nth-of-type(2) label:nth-of-type(1) input', '201/123/45678']
	]);
	await click('form button.primary');
	await settle();

	// seeds data through the API with the session's own CSRF token
	const api = (method, path, body) =>
		evaluate(`(async () => {
			const { csrf_token } = await (await fetch('/api/session')).json();
			const response = await fetch(${JSON.stringify(path)}, {
				method: ${JSON.stringify(method)},
				headers: { 'content-type': 'application/json', 'x-csrf-token': csrf_token },
				body: ${JSON.stringify(body === undefined ? null : JSON.stringify(body))}
			});
			return response.status === 204 ? null : response.json();
		})()`);
	const krume = await api('POST', '/api/customers', {
		name: 'Bäckerei Krume',
		language: 'de',
		email: 'info@krume.example',
		street: 'Brotgasse 3',
		postcode: '12345',
		city: 'Musterstadt',
		hourly_rate: 8500
	});
	const zurich = await api('POST', '/api/customers', { name: 'Atelier Zürich', language: 'en', currency: 'CHF', hourly_rate: 12000 });
	const relaunch = await api('POST', '/api/projects', { customer_id: krume.id, name: 'Website-Relaunch', hourly_rate: 9000 });
	await api('POST', '/api/projects', { customer_id: krume.id, name: 'Wartung', billable: false });
	const portal = await api('POST', '/api/projects', { customer_id: zurich.id, name: 'Kundenportal' });
	await api('POST', '/api/activities', { name: 'Beratung', rates: [{ currency: 'EUR', minor: 12000 }, { currency: 'CHF', minor: 15000 }] });
	const todos = [];
	for (const [project, title, status_id, priority, due_date] of [
		[relaunch, 'Impressum und Datenschutz prüfen', 1, 'high', '2026-01-08'],
		[relaunch, 'Fotos der Filialen einsammeln', 1, 'normal', '2030-11-14'],
		[portal, 'Angebot Wartungsvertrag vorbereiten', 1, 'low', null],
		[relaunch, 'Startseite überarbeiten', 2, 'high', '2030-11-12'],
		[portal, 'Sendungsverfolgung an die API anbinden', 2, 'normal', null],
		[relaunch, 'Freigabe der Texte durch die Kundin', 3, 'normal', null],
		[relaunch, 'Domain umziehen', 4, 'normal', null]
	]) {
		todos.push(await api('POST', '/api/todos', { project_id: project.id, title, status_id, priority, due_date, assignee_id: 1 }));
	}
	await api('POST', `/api/todos/${todos[3].id}/comments`, { body: 'Entwurf steht, Bilder fehlen noch.' });
	const earlier = Math.floor(Date.now() / 1000) - 4 * 3600;
	await api('POST', '/api/time', { project_id: relaunch.id, todo_id: todos[0].id, started_at: earlier - 5400, ended_at: earlier - 1800 });
	await api('POST', '/api/time', { project_id: portal.id, description: 'Abstimmung Texte', started_at: earlier, ended_at: earlier + 2700, billable: false });
	await api('POST', '/api/timers', { project_id: relaunch.id, todo_id: todos[3].id });
	await api('POST', '/api/timers', { project_id: portal.id, description: 'Telefonat' });

	const quote = await api('POST', '/api/documents', {
		kind: 'quote',
		customer_id: krume.id,
		project_id: relaunch.id,
		title: 'Relaunch der Website',
		intro: 'Vielen Dank für Ihre Anfrage. Gerne bieten wir an:',
		lines: [
			{ description: 'Konzeption und Gestaltung', quantity: '12', unit: 'hour', project_id: relaunch.id },
			{ description: 'Hosting (12 Monate)', quantity: '12', unit: 'month', unit_price: 1500 }
		]
	});
	await api('POST', `/api/documents/${quote.id}/issue`);
	const invoice = await api('POST', '/api/documents', {
		kind: 'invoice',
		customer_id: krume.id,
		lines: [{ description: 'Wartung im Oktober', quantity: '2.5', unit: 'hour', unit_price: 8500 }]
	});
	await api('POST', `/api/documents/${invoice.id}/time`, { group: 'todo', rounding: 15 });
	const issuedInvoice = await api('POST', '/api/documents', {
		kind: 'invoice',
		customer_id: krume.id,
		timesheet: true,
		lines: [{ description: 'Lizenz Kassensystem', quantity: '1', unit: 'piece', unit_price: 49000 }]
	});
	await api('POST', `/api/documents/${issuedInvoice.id}/issue`);
	await api('POST', `/api/documents/${issuedInvoice.id}/payments`, { date: '2026-11-01', amount: 20000 });
	await api('POST', '/api/snippets', { name: 'Zahlungshinweis', language: 'de', body: 'Bitte überweisen Sie den Betrag ohne Abzug.' });

	await screen('overview', `${origin}/`);
	await screen('customers', `${origin}/customers`);
	await screen('customer', `${origin}/customers/${krume.id}`);
	await screen('activities', `${origin}/activities`);
	await screen('statuses', `${origin}/statuses`);
	await screen('layouts', `${origin}/layouts`);
	await screen('snippets', `${origin}/snippets`);
	await screen('documents', `${origin}/documents`);
	await screen('document-draft', `${origin}/documents/${invoice.id}`);
	await screen('document-quote', `${origin}/documents/${quote.id}`);
	await screen('document-invoice', `${origin}/documents/${issuedInvoice.id}`);
	await screen('todos-board', `${origin}/todos`, () => evaluate("localStorage.setItem('todos.view', 'board')"));
	await screen('todos-list', `${origin}/todos`, async () => {
		await click('[role=radiogroup] [role=radio]');
		await settle();
	});
	await screen('todo', `${origin}/todos/${todos[3].id}`);
	await screen('time', `${origin}/time`);
	await screen('timers', `${origin}/time`, () => click('.timer-bar'), ['phone']);

	// drags a card for real: with the mouse on a desktop board, by long press and holding it at
	// the edge on a phone, then checks where the server put it
	const center = (selector) =>
		evaluate(`(() => {
			const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();
			return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
		})()`);
	const statusOf = async (todo) => (await api('GET', `/api/todos/${todo.id}`)).todo.status_id;

	await view('desktop', 'light');
	await open(`${origin}/todos`);
	await click('[role=radiogroup] [role=radio]:nth-child(2)');
	await settle();
	let from = await center(`[data-todo="${todos[0].id}"]`);
	let to = await center('[data-status="3"] h2');
	const mouse = (type, { x, y }) =>
		cdp.send('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons: type === 'mouseReleased' ? 0 : 1, clickCount: 1 });
	await mouse('mousePressed', from);
	for (let i = 1; i <= 10; i++) {
		await mouse('mouseMoved', { x: from.x + ((to.x - from.x) * i) / 10, y: from.y + ((to.y + 40 - from.y) * i) / 10 });
		await sleep(30);
	}
	await mouse('mouseReleased', { x: to.x, y: to.y + 40 });
	await settle();
	if ((await statusOf(todos[0])) !== 3) failures.push('desktop board: dragging a card to "Wartet" did not move it');

	await view('phone', 'light');
	await open(`${origin}/todos`);
	from = await center(`[data-todo="${todos[1].id}"]`);
	const touch = (type, points) => cdp.send('Input.dispatchTouchEvent', { type, touchPoints: points });
	await touch('touchStart', [from]);
	await sleep(500);
	const edge = { x: widths.phone.width - 10, y: from.y };
	for (let i = 1; i <= 8; i++) {
		await touch('touchMove', [{ x: from.x + ((edge.x - from.x) * i) / 8, y: from.y }]);
		await sleep(30);
	}
	await shoot('board-drag', 'phone', 'light');
	await sleep(800);
	await touch('touchMove', [{ x: widths.phone.width / 2, y: from.y + 20 }]);
	await sleep(100);
	await touch('touchEnd', []);
	await settle();
	if ((await statusOf(todos[1])) !== 2) failures.push('phone board: holding a card at the right edge did not move it to "In Arbeit"');
	await screen('business', `${origin}/business`);
	await screen('audit', `${origin}/audit`);
	await screen('account', `${origin}/account`);
	await screen('more', `${origin}/`, () => click('.bar button'), ['phone']);

	let invitation;
	await screen('users', `${origin}/users`, async () => {
		await fill([['form.invite input', 'Lena Meyer']]);
		await click('form.invite button.primary');
		invitation = await until('the invitation link', () => evaluate("document.querySelector('dialog[open] code')?.textContent"));
	});
	await screen('users-list', `${origin}/users`);

	await cdp.send('Network.clearBrowserCookies');
	await screen('sign-in', `${origin}/sign-in`);
	await screen('invitation', invitation);

	cdp.close();
	console.log(`screenshots in ${out}`);
} catch (error) {
	failures.push(String(error.stack ?? error));
} finally {
	children.forEach((child) => child.kill());
}

if (failures.length) {
	console.error(failures.join('\n'));
	process.exit(1);
}
