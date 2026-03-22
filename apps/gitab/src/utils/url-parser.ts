import { type GitLabResource, RESOURCE_CONFIGS } from './resource-types';

interface RoutePattern {
  /** Regex applied to the pathname portion after `/-/` */
  pattern: RegExp;
  type: GitLabResource['type'];
  /** Function to extract the ID from regex match groups */
  extractId?: (match: RegExpMatchArray) => string | undefined;
}

/**
 * Patterns for group-level resources (URLs starting with /groups/...)
 * These are matched before project-level patterns.
 */
const GROUP_PATTERNS: RoutePattern[] = [
  {
    pattern: /^\/groups\/(.+)\/-\/epics\/(\d+)/,
    type: 'epic',
    extractId: (m) => m[2],
  },
  {
    pattern: /^\/groups\/(.+)\/-\/milestones\/(\d+)/,
    type: 'milestone',
    extractId: (m) => m[2],
  },
];

/**
 * Patterns matched against the segment after `/-/` in the URL.
 * Order matters: more specific patterns first.
 */
const RESOURCE_PATTERNS: RoutePattern[] = [
  {
    pattern: /^issues\/(\d+)/,
    type: 'issue',
    extractId: (m) => m[1],
  },
  {
    pattern: /^issues\/?$/,
    type: 'issue',
  },
  {
    pattern: /^merge_requests\/(\d+)/,
    type: 'merge_request',
    extractId: (m) => m[1],
  },
  {
    pattern: /^merge_requests\/?$/,
    type: 'merge_request',
  },
  {
    pattern: /^milestones\/(\d+)/,
    type: 'milestone',
    extractId: (m) => m[1],
  },
  {
    pattern: /^milestones\/?$/,
    type: 'milestone',
  },
  {
    pattern: /^commit\/([0-9a-f]{7,40})/,
    type: 'commit',
    extractId: (m) => m[1],
  },
  {
    pattern: /^pipelines\/(\d+)/,
    type: 'pipeline',
    extractId: (m) => m[1],
  },
  {
    pattern: /^pipelines\/?$/,
    type: 'pipeline',
  },
  {
    pattern: /^jobs\/(\d+)/,
    type: 'job',
    extractId: (m) => m[1],
  },
  {
    pattern: /^jobs\/?$/,
    type: 'job',
  },
  {
    pattern: /^snippets\/(\d+)/,
    type: 'snippet',
    extractId: (m) => m[1],
  },
  {
    pattern: /^snippets\/?$/,
    type: 'snippet',
  },
  {
    pattern: /^releases\/.+/,
    type: 'release',
  },
  {
    pattern: /^releases\/?$/,
    type: 'release',
  },
  {
    pattern: /^tags\/.+/,
    type: 'tag',
  },
  {
    pattern: /^tags\/?$/,
    type: 'tag',
  },
  {
    pattern: /^blob\/.+/,
    type: 'blob',
  },
  {
    pattern: /^tree\/(.+)/,
    type: 'tree',
  },
  {
    pattern: /^branches/,
    type: 'branch',
  },
  {
    pattern: /^wikis\/.*/,
    type: 'wiki',
  },
  {
    pattern: /^settings/,
    type: 'settings',
  },
  {
    pattern: /^labels/,
    type: 'labels',
  },
  {
    pattern: /^packages\/(\d+)/,
    type: 'packages',
    extractId: (m) => m[1],
  },
  {
    pattern: /^packages\/?$/,
    type: 'packages',
  },
  {
    pattern: /^environments\/(\d+)/,
    type: 'environments',
    extractId: (m) => m[1],
  },
  {
    pattern: /^environments\/?$/,
    type: 'environments',
  },
];

/**
 * Parse a GitLab URL pathname into a structured resource descriptor.
 *
 * @param pathname - The URL pathname (e.g. "/group/project/-/issues/123")
 * @returns A GitLabResource describing the type of resource and its label
 */
export function parseGitLabUrl(pathname: string): GitLabResource {
  // Strip trailing slash for consistency
  const path = pathname.replace(/\/+$/, '');

  // 1. Check group-level patterns first (e.g. /groups/.../-/epics/42)
  for (const route of GROUP_PATTERNS) {
    const match = path.match(route.pattern);
    if (match) {
      return buildResource(route, match);
    }
  }

  // 2. Find the `/-/` separator that divides namespace/project from resource
  const separatorIndex = path.indexOf('/-/');
  if (separatorIndex === -1) {
    // No `/-/` means this is either a project root, user profile, or unrecognized page.
    // If the path has at least two segments (namespace/project), treat as project root.
    const segments = path.split('/').filter(Boolean);
    if (segments.length >= 2) {
      return { type: 'project' };
    }
    return { type: 'unknown' };
  }

  // 3. Extract the resource portion after `/-/`
  const resourcePath = path.substring(separatorIndex + 3);

  // 4. Match against known resource patterns
  for (const route of RESOURCE_PATTERNS) {
    const match = resourcePath.match(route.pattern);
    if (match) {
      return buildResource(route, match);
    }
  }

  return { type: 'unknown' };
}

function buildResource(
  route: RoutePattern,
  match: RegExpMatchArray,
): GitLabResource {
  const config = RESOURCE_CONFIGS[route.type];
  const rawId = route.extractId?.(match);

  if (!rawId) {
    return { type: route.type };
  }

  // For commits, truncate SHA to 7 characters
  const id = route.type === 'commit' ? rawId.substring(0, 7) : rawId;
  const prefix = config.prefix;
  const label = prefix ? `${prefix}${id}` : undefined;

  return {
    type: route.type,
    id,
    prefix,
    label,
  };
}
