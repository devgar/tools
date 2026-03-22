import { describe, it, expect } from 'vitest';
import { parseGitLabUrl } from './url-parser';

describe('parseGitLabUrl', () => {
  describe('issues', () => {
    it('parses an issue with ID', () => {
      expect(parseGitLabUrl('/gitlab-org/gitlab/-/issues/123')).toEqual({
        type: 'issue',
        id: '123',
        prefix: '#',
        label: '#123',
      });
    });

    it('parses issue list page', () => {
      expect(parseGitLabUrl('/gitlab-org/gitlab/-/issues')).toEqual({
        type: 'issue',
      });
    });

    it('parses issue with nested subgroups', () => {
      expect(
        parseGitLabUrl('/group/sub1/sub2/project/-/issues/456'),
      ).toEqual({
        type: 'issue',
        id: '456',
        prefix: '#',
        label: '#456',
      });
    });

    it('parses issue with trailing slash', () => {
      expect(parseGitLabUrl('/group/project/-/issues/789/')).toEqual({
        type: 'issue',
        id: '789',
        prefix: '#',
        label: '#789',
      });
    });

    it('parses issue with fragment (note link)', () => {
      expect(
        parseGitLabUrl('/group/project/-/issues/100#note_12345'),
      ).toEqual({
        type: 'issue',
        id: '100',
        prefix: '#',
        label: '#100',
      });
    });
  });

  describe('merge requests', () => {
    it('parses a merge request with ID', () => {
      expect(
        parseGitLabUrl('/gitlab-org/gitlab/-/merge_requests/141'),
      ).toEqual({
        type: 'merge_request',
        id: '141',
        prefix: '!',
        label: '!141',
      });
    });

    it('parses merge request list page', () => {
      expect(
        parseGitLabUrl('/gitlab-org/gitlab/-/merge_requests'),
      ).toEqual({
        type: 'merge_request',
      });
    });
  });

  describe('milestones', () => {
    it('parses a project milestone with ID', () => {
      expect(
        parseGitLabUrl('/group/project/-/milestones/12'),
      ).toEqual({
        type: 'milestone',
        id: '12',
        prefix: '%',
        label: '%12',
      });
    });

    it('parses a group milestone', () => {
      expect(
        parseGitLabUrl('/groups/my-group/my-subgroup/-/milestones/50'),
      ).toEqual({
        type: 'milestone',
        id: '50',
        prefix: '%',
        label: '%50',
      });
    });

    it('parses milestone list page', () => {
      expect(
        parseGitLabUrl('/group/project/-/milestones'),
      ).toEqual({
        type: 'milestone',
      });
    });
  });

  describe('commits', () => {
    it('parses a commit with full SHA', () => {
      const result = parseGitLabUrl(
        '/group/project/-/commit/abc1234def5678901234567890abcdef12345678',
      );
      expect(result).toEqual({
        type: 'commit',
        id: 'abc1234',
        prefix: '@',
        label: '@abc1234',
      });
    });

    it('parses a commit with short SHA', () => {
      expect(
        parseGitLabUrl('/group/project/-/commit/abc1234'),
      ).toEqual({
        type: 'commit',
        id: 'abc1234',
        prefix: '@',
        label: '@abc1234',
      });
    });
  });

  describe('pipelines', () => {
    it('parses a pipeline with ID', () => {
      expect(
        parseGitLabUrl('/group/project/-/pipelines/98765'),
      ).toEqual({
        type: 'pipeline',
        id: '98765',
      });
    });

    it('parses pipeline list page', () => {
      expect(
        parseGitLabUrl('/group/project/-/pipelines'),
      ).toEqual({
        type: 'pipeline',
      });
    });
  });

  describe('jobs', () => {
    it('parses a job with ID', () => {
      expect(parseGitLabUrl('/group/project/-/jobs/567')).toEqual({
        type: 'job',
        id: '567',
      });
    });

    it('parses job list page', () => {
      expect(parseGitLabUrl('/group/project/-/jobs')).toEqual({
        type: 'job',
      });
    });
  });

  describe('snippets', () => {
    it('parses a snippet with ID', () => {
      expect(
        parseGitLabUrl('/group/project/-/snippets/89'),
      ).toEqual({
        type: 'snippet',
        id: '89',
        prefix: '$',
        label: '$89',
      });
    });

    it('parses snippet list page', () => {
      expect(
        parseGitLabUrl('/group/project/-/snippets'),
      ).toEqual({
        type: 'snippet',
      });
    });
  });

  describe('epics', () => {
    it('parses a group epic with ID', () => {
      expect(
        parseGitLabUrl('/groups/gitlab-org/-/epics/45'),
      ).toEqual({
        type: 'epic',
        id: '45',
        prefix: '&',
        label: '&45',
      });
    });

    it('parses a nested group epic', () => {
      expect(
        parseGitLabUrl('/groups/gitlab-org/sub/-/epics/100'),
      ).toEqual({
        type: 'epic',
        id: '100',
        prefix: '&',
        label: '&100',
      });
    });
  });

  describe('branches and tags', () => {
    it('parses a branch (tree) view', () => {
      expect(
        parseGitLabUrl('/group/project/-/tree/main'),
      ).toEqual({
        type: 'tree',
      });
    });

    it('parses branches list page', () => {
      expect(
        parseGitLabUrl('/group/project/-/branches'),
      ).toEqual({
        type: 'branch',
      });
    });

    it('parses a specific tag', () => {
      expect(
        parseGitLabUrl('/group/project/-/tags/v1.0.0'),
      ).toEqual({
        type: 'tag',
      });
    });

    it('parses tags list page', () => {
      expect(
        parseGitLabUrl('/group/project/-/tags'),
      ).toEqual({
        type: 'tag',
      });
    });
  });

  describe('releases', () => {
    it('parses a specific release', () => {
      expect(
        parseGitLabUrl('/group/project/-/releases/v1.0.0'),
      ).toEqual({
        type: 'release',
      });
    });

    it('parses releases list page', () => {
      expect(
        parseGitLabUrl('/group/project/-/releases'),
      ).toEqual({
        type: 'release',
      });
    });
  });

  describe('files', () => {
    it('parses a blob (file) view', () => {
      expect(
        parseGitLabUrl('/group/project/-/blob/main/README.md'),
      ).toEqual({
        type: 'blob',
      });
    });

    it('parses a tree (directory) view', () => {
      expect(
        parseGitLabUrl('/group/project/-/tree/main/src/components'),
      ).toEqual({
        type: 'tree',
      });
    });
  });

  describe('wiki', () => {
    it('parses a wiki page', () => {
      expect(
        parseGitLabUrl('/group/project/-/wikis/home'),
      ).toEqual({
        type: 'wiki',
      });
    });
  });

  describe('settings', () => {
    it('parses a settings page', () => {
      expect(
        parseGitLabUrl('/group/project/-/settings/ci_cd'),
      ).toEqual({
        type: 'settings',
      });
    });
  });

  describe('labels', () => {
    it('parses labels page', () => {
      expect(
        parseGitLabUrl('/group/project/-/labels'),
      ).toEqual({
        type: 'labels',
      });
    });
  });

  describe('packages', () => {
    it('parses a specific package', () => {
      expect(
        parseGitLabUrl('/group/project/-/packages/42'),
      ).toEqual({
        type: 'packages',
        id: '42',
      });
    });

    it('parses packages list page', () => {
      expect(
        parseGitLabUrl('/group/project/-/packages'),
      ).toEqual({
        type: 'packages',
      });
    });
  });

  describe('environments', () => {
    it('parses a specific environment', () => {
      expect(
        parseGitLabUrl('/group/project/-/environments/7'),
      ).toEqual({
        type: 'environments',
        id: '7',
      });
    });

    it('parses environments list page', () => {
      expect(
        parseGitLabUrl('/group/project/-/environments'),
      ).toEqual({
        type: 'environments',
      });
    });
  });

  describe('project root', () => {
    it('parses a project root page', () => {
      expect(parseGitLabUrl('/gitlab-org/gitlab')).toEqual({
        type: 'project',
      });
    });

    it('parses a project root with trailing slash', () => {
      expect(parseGitLabUrl('/gitlab-org/gitlab/')).toEqual({
        type: 'project',
      });
    });

    it('parses a nested group project root', () => {
      expect(
        parseGitLabUrl('/group/subgroup/project'),
      ).toEqual({
        type: 'project',
      });
    });
  });

  describe('unknown / edge cases', () => {
    it('returns unknown for a single-segment path (user profile)', () => {
      expect(parseGitLabUrl('/username')).toEqual({
        type: 'unknown',
      });
    });

    it('returns unknown for root path', () => {
      expect(parseGitLabUrl('/')).toEqual({
        type: 'unknown',
      });
    });

    it('returns unknown for unrecognized resource after /-/', () => {
      expect(
        parseGitLabUrl('/group/project/-/something_unknown'),
      ).toEqual({
        type: 'unknown',
      });
    });

    it('handles query strings without breaking', () => {
      expect(
        parseGitLabUrl('/group/project/-/issues/42?scope=all'),
      ).toEqual({
        type: 'issue',
        id: '42',
        prefix: '#',
        label: '#42',
      });
    });
  });
});
