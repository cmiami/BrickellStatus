import { redirect } from '@sveltejs/kit';

// Outputs moved under System. Kept so an existing link, or the tray, still
// lands somewhere rather than on a blank page.
export function load() {
  redirect(307, '/system/outputs');
}
