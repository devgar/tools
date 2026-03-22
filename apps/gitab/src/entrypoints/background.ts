import { getDomains } from '@/utils/storage';

export default defineBackground(() => {
  // On install/update: register content scripts for all configured domains
  browser.runtime.onInstalled.addListener(async () => {
    await registerContentScripts();
  });

  // On startup: ensure content scripts are registered
  browser.runtime.onStartup.addListener(async () => {
    await registerContentScripts();
  });

  // Listen for storage changes to sync registered content scripts
  browser.storage.onChanged.addListener(async (changes, areaName) => {
    if (areaName === 'sync' && changes['gitab:domains']) {
      await registerContentScripts();
    }
  });
});

async function registerContentScripts(): Promise<void> {
  const domains = await getDomains();

  // Build match patterns for all domains
  const matches = domains.map((d) => `*://*.${d}/*`);

  // Unregister existing dynamic scripts first
  try {
    await browser.scripting.unregisterContentScripts({
      ids: ['gitab-dynamic'],
    });
  } catch {
    // Ignore if not registered yet
  }

  // Register content scripts for all configured domains
  // The static content script only covers gitlab.com — this covers self-hosted
  const selfHostedDomains = domains.filter((d) => d !== 'gitlab.com');
  if (selfHostedDomains.length === 0) return;

  const selfHostedMatches = selfHostedDomains.map((d) => `*://*.${d}/*`);

  try {
    await browser.scripting.registerContentScripts([
      {
        id: 'gitab-dynamic',
        matches: selfHostedMatches,
        js: ['content-scripts/gitlab.js'],
        runAt: 'document_end',
      },
    ]);
  } catch (error) {
    console.error('[gitab] Failed to register content scripts:', error);
  }
}
