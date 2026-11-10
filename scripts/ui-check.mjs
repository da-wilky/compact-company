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
		['form label:nth-of-type(1) input', 'Beispiel GmbH'],
		['form textarea', 'Hauptstraße 1\n12345 Musterstadt']
	]);
	await click('form button.primary');
	await settle();

	await screen('overview', `${origin}/`);
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
