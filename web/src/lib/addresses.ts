import type { Schemas } from '#lib/api/index.ts';

type Address = Schemas['CustomerAddress'];

export function addressLines(address: Address): string {
	return [address.street, [address.postcode, address.city].filter(Boolean).join(' '), address.country]
		.filter(Boolean)
		.join('\n');
}

export function addressSummary(address: Address): string {
	const place = [address.street?.split('\n').at(-1), address.city].filter(Boolean).join(', ');
	return [address.label, place].filter(Boolean).join(' – ') || address.country;
}
