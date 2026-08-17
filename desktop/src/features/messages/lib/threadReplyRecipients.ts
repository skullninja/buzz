import { normalizeMentionPubkeys } from "./threading";

/**
 * Recipients for a threaded reply: everyone explicitly mentioned, plus the
 * author of the message being replied to.
 *
 * Replying to someone notifies them. Without this, a reply inside a thread
 * carries no `p` tag for its recipient, so a mention-filtered subscriber —
 * an ACP agent, a notification rule — never sees it, and the person you are
 * replying to has to be re-mentioned by name to be reached at all.
 *
 * Self is always dropped, and an author already mentioned is not duplicated.
 */
export function threadReplyRecipients(
  explicit: readonly string[],
  parentAuthor: string | undefined,
  selfPubkey: string,
): string[] {
  const recipients = normalizeMentionPubkeys([...explicit], selfPubkey);
  if (!parentAuthor) return recipients;

  const [author] = normalizeMentionPubkeys([parentAuthor], selfPubkey);
  if (!author || recipients.includes(author)) return recipients;

  return [...recipients, author];
}
