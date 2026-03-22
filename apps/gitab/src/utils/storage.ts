const STORAGE_KEY = 'gitab:domains';
const DEFAULT_DOMAINS = ['gitlab.com'];

/**
 * Get all configured GitLab domains (always includes gitlab.com).
 */
export async function getDomains(): Promise<string[]> {
  const result = await browser.storage.sync.get(STORAGE_KEY);
  const stored: string[] = result[STORAGE_KEY] ?? [];
  // Ensure gitlab.com is always present
  const domains = new Set([...DEFAULT_DOMAINS, ...stored]);
  return [...domains];
}

/**
 * Add a new GitLab domain to the configuration.
 * Returns true if added, false if already present.
 */
export async function addDomain(domain: string): Promise<boolean> {
  const normalized = normalizeDomain(domain);
  if (!normalized) return false;

  const domains = await getDomains();
  if (domains.includes(normalized)) return false;

  domains.push(normalized);
  await browser.storage.sync.set({ [STORAGE_KEY]: domains });
  return true;
}

/**
 * Remove a GitLab domain from the configuration.
 * Cannot remove gitlab.com.
 * Returns true if removed, false if not found or is default.
 */
export async function removeDomain(domain: string): Promise<boolean> {
  const normalized = normalizeDomain(domain);
  if (!normalized || DEFAULT_DOMAINS.includes(normalized)) return false;

  const domains = await getDomains();
  const filtered = domains.filter((d) => d !== normalized);
  if (filtered.length === domains.length) return false;

  await browser.storage.sync.set({ [STORAGE_KEY]: filtered });
  return true;
}

/**
 * Normalize a domain string: lowercase, strip protocol and paths.
 */
function normalizeDomain(input: string): string | null {
  const trimmed = input.trim().toLowerCase();
  if (!trimmed) return null;

  // Strip protocol if present
  const withoutProtocol = trimmed
    .replace(/^https?:\/\//, '')
    .replace(/\/.*$/, '');

  // Basic hostname validation
  if (!/^[a-z0-9]([a-z0-9.-]*[a-z0-9])?$/.test(withoutProtocol)) {
    return null;
  }

  return withoutProtocol;
}
