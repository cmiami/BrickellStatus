import { redirect } from '@sveltejs/kit';

// Alert delivery and quiet hours are global output concerns now. Channel-level
// timing, repeat and interrupt controls no longer exist.
export function load() {
  redirect(307, '/system/outputs');
}
