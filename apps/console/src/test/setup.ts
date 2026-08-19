import '@testing-library/jest-dom/vitest';
import { readable } from 'svelte/store';
import { vi } from 'vitest';

// SvelteKit fills `$app/stores` in from the router, which does not exist under
// jsdom. Components that read the current path — the System tabs, for one —
// would otherwise fail on `$page.url` in every test that happens to mount one.
vi.mock('$app/stores', () => ({
  page: readable({ url: new URL('http://localhost/'), params: {}, route: { id: null } })
}));
