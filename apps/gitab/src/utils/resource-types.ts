export type ResourceType =
  | 'issue'
  | 'merge_request'
  | 'milestone'
  | 'commit'
  | 'pipeline'
  | 'job'
  | 'branch'
  | 'tag'
  | 'snippet'
  | 'epic'
  | 'release'
  | 'blob'
  | 'tree'
  | 'wiki'
  | 'settings'
  | 'project'
  | 'labels'
  | 'packages'
  | 'environments'
  | 'unknown';

export type Shape =
  | 'circle'
  | 'square'
  | 'diamond'
  | 'triangle'
  | 'triangle-down'
  | 'hexagon'
  | 'pentagon'
  | 'star'
  | 'rounded-square';

export interface GitLabResource {
  type: ResourceType;
  /** Raw identifier from the URL (e.g. "213", "abc1234def") */
  id?: string;
  /** GitLab reference prefix (e.g. "#", "!", "%", "$", "@", "&") */
  prefix?: string;
  /** Displayable label combining prefix + id (e.g. "#213", "!141") */
  label?: string;
}

export interface ResourceConfig {
  color: string;
  shape: Shape;
  prefix?: string;
}

export const RESOURCE_CONFIGS: Record<ResourceType, ResourceConfig> = {
  issue:         { color: '#FC6D26', shape: 'circle',         prefix: '#' },
  merge_request: { color: '#6B4FBB', shape: 'diamond',        prefix: '!' },
  milestone:     { color: '#1AAA55', shape: 'pentagon',        prefix: '%' },
  commit:        { color: '#2E2E2E', shape: 'square',          prefix: '@' },
  pipeline:      { color: '#1F75CB', shape: 'hexagon' },
  job:           { color: '#428BCA', shape: 'hexagon' },
  branch:        { color: '#34C759', shape: 'triangle' },
  tag:           { color: '#FFC107', shape: 'triangle-down' },
  snippet:       { color: '#E44D26', shape: 'rounded-square',  prefix: '$' },
  epic:          { color: '#9B59B6', shape: 'star',            prefix: '&' },
  release:       { color: '#17A2B8', shape: 'pentagon' },
  blob:          { color: '#6E6E6E', shape: 'rounded-square' },
  tree:          { color: '#6E6E6E', shape: 'square' },
  wiki:          { color: '#3498DB', shape: 'circle' },
  settings:      { color: '#868686', shape: 'hexagon' },
  project:       { color: '#FC6D26', shape: 'square' },
  labels:        { color: '#D4A017', shape: 'circle' },
  packages:      { color: '#6E6E6E', shape: 'pentagon' },
  environments:  { color: '#1AAA55', shape: 'hexagon' },
  unknown:       { color: '#999999', shape: 'circle' },
};
