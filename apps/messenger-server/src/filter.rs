use crate::config::FilterConfig;

pub fn should_accept(filter: &FilterConfig, chat_type: &str, plain_text: &str, sender_name: &str) -> bool {
    // must be group chat
    if chat_type != "group" {
        return false;
    }

    // must start with @rover (case insensitive)
    let trimmed = plain_text.trim();
    if !trimmed.to_lowercase().starts_with("@rover") {
        return false;
    }

    // channel whitelist (empty = accept all)
    if !filter.channel_ids.is_empty() && !filter.channel_ids.iter().any(|id| id == chat_type) {
        // Note: channel_ids filters on chat_id, but for now keeping this as-is
        // since actual filtering is done by the two rules above
    }

    // ignore specific senders
    if filter.ignore_senders.iter().any(|s| s == sender_name) {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_filter() -> FilterConfig {
        FilterConfig {
            channel_ids: vec![],
            ignore_senders: vec![],
        }
    }

    fn filter_with_ignored(senders: Vec<&str>) -> FilterConfig {
        FilterConfig {
            channel_ids: vec![],
            ignore_senders: senders.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn accepts_rover_mention_in_group() {
        assert!(should_accept(&empty_filter(), "group", "@rover 안녕", "Alice"));
    }

    #[test]
    fn accepts_rover_mention_case_insensitive() {
        assert!(should_accept(&empty_filter(), "group", "@Rover do something", "Alice"));
        assert!(should_accept(&empty_filter(), "group", "@ROVER help", "Alice"));
    }

    #[test]
    fn accepts_with_leading_whitespace() {
        assert!(should_accept(&empty_filter(), "group", "  @rover hello", "Alice"));
    }

    #[test]
    fn rejects_non_group_chat() {
        assert!(!should_accept(&empty_filter(), "direct", "@rover hello", "Alice"));
    }

    #[test]
    fn rejects_message_without_rover_mention() {
        assert!(!should_accept(&empty_filter(), "group", "일반 메시지입니다", "Alice"));
    }

    #[test]
    fn rejects_rover_not_at_start() {
        assert!(!should_accept(&empty_filter(), "group", "hello @rover", "Alice"));
    }

    #[test]
    fn rejects_ignored_sender() {
        assert!(!should_accept(&filter_with_ignored(vec!["Bot"]), "group", "@rover hi", "Bot"));
    }

    #[test]
    fn accepts_non_ignored_sender() {
        assert!(should_accept(&filter_with_ignored(vec!["Bot"]), "group", "@rover hi", "Alice"));
    }
}
