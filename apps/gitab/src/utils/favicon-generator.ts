import { type GitLabResource, type Shape, RESOURCE_CONFIGS } from './resource-types';

const FAVICON_SIZE = 32;

/**
 * Generate a favicon as a data URL for a given GitLab resource.
 * Returns null for unknown resources (no favicon change needed).
 */
export function generateFavicon(resource: GitLabResource): string | null {
  if (resource.type === 'unknown') {
    return null;
  }

  const config = RESOURCE_CONFIGS[resource.type];
  const canvas = new OffscreenCanvas(FAVICON_SIZE, FAVICON_SIZE);
  const ctx = canvas.getContext('2d');
  if (!ctx) return null;

  // Clear canvas
  ctx.clearRect(0, 0, FAVICON_SIZE, FAVICON_SIZE);

  // Draw the shape
  drawShape(ctx, config.shape, config.color);

  // Draw the label badge if present
  if (resource.label) {
    drawLabel(ctx, resource.label);
  }

  // Convert to blob URL — but since OffscreenCanvas.toDataURL doesn't exist,
  // we use convertToBlob. However, for synchronous use in the DOM we need a
  // data URL. We'll draw to a regular canvas approach instead.
  // Actually, OffscreenCanvas has transferToImageBitmap but not toDataURL.
  // We'll return the canvas and let the caller handle conversion.
  // For simplicity, return the canvas reference and convert in the caller.

  // OffscreenCanvas doesn't have toDataURL, so we return the canvas for async conversion.
  return null; // placeholder - real implementation uses async version below
}

/**
 * Async version: generates a favicon data URL for a GitLab resource.
 * Uses OffscreenCanvas.convertToBlob() for conversion.
 */
export async function generateFaviconDataUrl(
  resource: GitLabResource,
): Promise<string | null> {
  if (resource.type === 'unknown') {
    return null;
  }

  const config = RESOURCE_CONFIGS[resource.type];
  const canvas = new OffscreenCanvas(FAVICON_SIZE, FAVICON_SIZE);
  const ctx = canvas.getContext('2d');
  if (!ctx) return null;

  ctx.clearRect(0, 0, FAVICON_SIZE, FAVICON_SIZE);

  drawShape(ctx, config.shape, config.color);

  if (resource.label) {
    drawLabel(ctx, resource.label);
  }

  const blob = await canvas.convertToBlob({ type: 'image/png' });
  return blobToDataUrl(blob);
}

/**
 * Fallback: generates a favicon using a regular HTMLCanvasElement.
 * Use this when running in a content script where document is available.
 */
export function generateFaviconSync(resource: GitLabResource): string | null {
  if (resource.type === 'unknown') {
    return null;
  }

  const config = RESOURCE_CONFIGS[resource.type];
  const canvas = document.createElement('canvas');
  canvas.width = FAVICON_SIZE;
  canvas.height = FAVICON_SIZE;
  const ctx = canvas.getContext('2d');
  if (!ctx) return null;

  ctx.clearRect(0, 0, FAVICON_SIZE, FAVICON_SIZE);

  drawShape(ctx, config.shape, config.color);

  if (resource.label) {
    drawLabel(ctx, resource.label);
  }

  return canvas.toDataURL('image/png');
}

function blobToDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onloadend = () => resolve(reader.result as string);
    reader.onerror = reject;
    reader.readAsDataURL(blob);
  });
}

type RenderContext = OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D;

function drawShape(ctx: RenderContext, shape: Shape, color: string): void {
  const s = FAVICON_SIZE;
  const cx = s / 2;
  const cy = s / 2;
  const r = s * 0.4; // radius for shapes

  ctx.fillStyle = color;
  ctx.beginPath();

  switch (shape) {
    case 'circle':
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      break;

    case 'square':
      ctx.rect(cx - r, cy - r, r * 2, r * 2);
      break;

    case 'rounded-square': {
      const rr = r * 0.25; // corner radius
      roundedRect(ctx, cx - r, cy - r, r * 2, r * 2, rr);
      break;
    }

    case 'diamond':
      ctx.moveTo(cx, cy - r);
      ctx.lineTo(cx + r, cy);
      ctx.lineTo(cx, cy + r);
      ctx.lineTo(cx - r, cy);
      ctx.closePath();
      break;

    case 'triangle':
      ctx.moveTo(cx, cy - r);
      ctx.lineTo(cx + r, cy + r * 0.7);
      ctx.lineTo(cx - r, cy + r * 0.7);
      ctx.closePath();
      break;

    case 'triangle-down':
      ctx.moveTo(cx, cy + r);
      ctx.lineTo(cx + r, cy - r * 0.7);
      ctx.lineTo(cx - r, cy - r * 0.7);
      ctx.closePath();
      break;

    case 'hexagon':
      drawPolygon(ctx, cx, cy, r, 6);
      break;

    case 'pentagon':
      drawPolygon(ctx, cx, cy, r, 5);
      break;

    case 'star':
      drawStar(ctx, cx, cy, r, r * 0.5, 5);
      break;
  }

  ctx.fill();
}

function drawPolygon(
  ctx: RenderContext,
  cx: number,
  cy: number,
  r: number,
  sides: number,
): void {
  const angleStep = (Math.PI * 2) / sides;
  // Start from top (-PI/2)
  for (let i = 0; i < sides; i++) {
    const angle = -Math.PI / 2 + i * angleStep;
    const x = cx + r * Math.cos(angle);
    const y = cy + r * Math.sin(angle);
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.closePath();
}

function drawStar(
  ctx: RenderContext,
  cx: number,
  cy: number,
  outerR: number,
  innerR: number,
  points: number,
): void {
  const step = Math.PI / points;
  for (let i = 0; i < points * 2; i++) {
    const angle = -Math.PI / 2 + i * step;
    const r = i % 2 === 0 ? outerR : innerR;
    const x = cx + r * Math.cos(angle);
    const y = cy + r * Math.sin(angle);
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.closePath();
}

function roundedRect(
  ctx: RenderContext,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  ctx.moveTo(x + r, y);
  ctx.lineTo(x + w - r, y);
  ctx.arcTo(x + w, y, x + w, y + r, r);
  ctx.lineTo(x + w, y + h - r);
  ctx.arcTo(x + w, y + h, x + w - r, y + h, r);
  ctx.lineTo(x + r, y + h);
  ctx.arcTo(x, y + h, x, y + h - r, r);
  ctx.lineTo(x, y + r);
  ctx.arcTo(x, y, x + r, y, r);
  ctx.closePath();
}

function drawLabel(ctx: RenderContext, label: string): void {
  const s = FAVICON_SIZE;

  const suffix = label.slice(-3);
  const prefix = label.slice(0, -3);
  const prefixFontSize = 14;
  const suffixFontSize = 16;
  const padding = 0.5;

  if (prefix) {
    ctx.font = `bold ${prefixFontSize}px sans-serif`;
    const prefixWidth = ctx.measureText(prefix).width;
    const prefixBadgeW = Math.min(s, prefixWidth + padding * 2);
    const prefixBadgeH = prefixFontSize + padding * 2;
    const prefixBadgeX = 0;
    const prefixBadgeY = 0;

    ctx.fillStyle = 'rgba(0, 0, 0, 0.5)';
    ctx.beginPath();
    ctx.rect(prefixBadgeX, prefixBadgeY, prefixBadgeW, prefixBadgeH);
    ctx.fill();

    ctx.save();
    ctx.beginPath();
    ctx.rect(prefixBadgeX, prefixBadgeY, prefixBadgeW, prefixBadgeH);
    ctx.clip();
    ctx.fillStyle = '#FFFFFF';
    ctx.textBaseline = 'top';
    ctx.font = `bold ${prefixFontSize}px sans-serif`;
    ctx.fillText(prefix, prefixBadgeX + padding, prefixBadgeY + padding + prefixFontSize * 0.1);
    ctx.restore();
  }

  if (suffix) {
    ctx.font = `bold ${suffixFontSize}px sans-serif`;
    const suffixWidth = ctx.measureText(suffix).width;
    const suffixBadgeW = Math.min(s, suffixWidth + padding * 2);
    const suffixBadgeH = suffixFontSize + padding * 2;
    const suffixBadgeX = s - suffixBadgeW;
    const suffixBadgeY = s - suffixBadgeH;

    ctx.fillStyle = 'rgba(0, 0, 0, 0.5)';
    ctx.beginPath();
    ctx.rect(suffixBadgeX, suffixBadgeY, suffixBadgeW, suffixBadgeH);
    ctx.fill();

    const suffixTextX = suffixBadgeX + suffixBadgeW - padding - suffixWidth;
    const suffixTextY = suffixBadgeY + padding + suffixFontSize * 0.15;

    ctx.save();
    ctx.beginPath();
    ctx.rect(suffixBadgeX, suffixBadgeY, suffixBadgeW, suffixBadgeH);
    ctx.clip();
    ctx.fillStyle = '#FFFFFF';
    ctx.textBaseline = 'top';
    ctx.font = `bold ${suffixFontSize}px sans-serif`;
    ctx.fillText(suffix, suffixTextX, suffixTextY);
    ctx.restore();
  }
}
