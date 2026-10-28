import { convertFileSrc } from '@tauri-apps/api/core';
import {
  baseName,
  dirName,
  formatBytes,
  formatCount,
  formatFullTime,
  isPreviewableImage,
  type ClipboardDetail,
} from '../lib/clipboard';

interface PreviewPaneProps {
  detail: ClipboardDetail | null;
  loading: boolean;
}

const MAX_PREVIEW_CHARS = 2000;

/** 右侧预览区（280px，UI-SPEC §4.4） */
export function PreviewPane({ detail, loading }: PreviewPaneProps) {
  if (loading) {
    return (
      <aside className="qw-preview" aria-label="内容预览">
        <p className="qw-preview-placeholder">正在读取…</p>
      </aside>
    );
  }

  if (!detail) {
    return (
      <aside className="qw-preview" aria-label="内容预览">
        <p className="qw-preview-placeholder">选择左侧条目查看内容</p>
      </aside>
    );
  }

  return (
    <aside className="qw-preview" aria-label="内容预览">
      <div className="qw-preview-body">{renderBody(detail)}</div>

      <div className="qw-preview-meta">
        <span>{detail.source_app || '未知来源'}</span>
        <span>{formatFullTime(detail.created_at)}</span>
        <span>{formatBytes(detail.byte_size)}</span>
        {detail.copy_count > 1 && <span>已复制 {formatCount(detail.copy_count)} 次</span>}
        {detail.truncated && <span className="qw-warn">已截断</span>}
      </div>
    </aside>
  );
}

function renderBody(detail: ClipboardDetail) {
  switch (detail.kind) {
    case 'image':
      return renderImage(detail);
    case 'files':
      return renderFiles(detail);
    default:
      return renderText(detail);
  }
}

function renderText(detail: ClipboardDetail) {
  const text = detail.text_body ?? detail.html ?? detail.rtf ?? detail.preview;
  const truncated = text.length > MAX_PREVIEW_CHARS;

  return (
    <>
      <pre className="qw-preview-text">
        {truncated ? text.slice(0, MAX_PREVIEW_CHARS) : text}
      </pre>
      {truncated && <p className="qw-preview-note">内容较长，已折叠</p>}
    </>
  );
}

function renderImage(detail: ClipboardDetail) {
  if (!detail.blob_path) {
    return <p className="qw-preview-placeholder">原图过大，未保存原图</p>;
  }

  // DIB 无法被 WebView 解码：不发出必然失败的 img 请求
  if (!isPreviewableImage(detail.blob_path)) {
    return <p className="qw-preview-placeholder">该图片格式暂不支持预览</p>;
  }

  return (
    <div className="qw-preview-image">
      <img
        src={convertFileSrc(detail.blob_path)}
        alt="剪贴板图片预览"
        className="qw-preview-img"
      />
    </div>
  );
}

function renderFiles(detail: ClipboardDetail) {
  const files = detail.file_list ?? [];
  const exists = detail.file_exists ?? [];

  if (files.length === 0) {
    return <p className="qw-preview-placeholder">没有文件列表</p>;
  }

  return (
    <ul className="qw-preview-files">
      {files.map((path, index) => {
        const missing = exists[index] === false;
        return (
          <li key={`${path}-${index}`} className={missing ? 'qw-missing' : undefined}>
            <span className="qw-file-name">{baseName(path)}</span>
            <span className="qw-file-dir">{dirName(path)}</span>
            {missing && <span className="qw-warn">文件已不存在</span>}
          </li>
        );
      })}
    </ul>
  );
}
