use moss_core::slot::prompt_preview;

#[test]
fn plain_prompts_show_their_first_line() {
    assert_eq!(
        prompt_preview("\n  fix the login page \nthen tests").as_deref(),
        Some("fix the login page")
    );
}

#[test]
fn messages_wrapped_in_markup_show_what_was_said() {
    let msg =
        "<cross-session-message from=\"uds:/tmp/x.sock\" from-name=\"api\" from-mode=\"bypass\">\n\
               Please rebase onto main\n</cross-session-message>";
    assert_eq!(
        prompt_preview(msg).as_deref(),
        Some("Please rebase onto main")
    );
    let inline = "<command-name>/review</command-name>";
    assert_eq!(prompt_preview(inline).as_deref(), Some("/review"));
}

#[test]
fn markup_with_nothing_inside_shows_nothing() {
    // What older versions stored: the opening tag alone.
    assert_eq!(
        prompt_preview("<cross-session-message from=\"uds:/tmp/x.sock\" from-mode=\"bypass\">"),
        None
    );
    assert_eq!(prompt_preview("   \n"), None);
}

#[test]
fn text_that_merely_mentions_angle_brackets_is_kept() {
    assert_eq!(
        prompt_preview("why does a < b fail here?").as_deref(),
        Some("why does a < b fail here?")
    );
    assert_eq!(
        prompt_preview("<div> is not centered").as_deref(),
        Some("<div> is not centered")
    );
}

#[test]
fn previews_are_at_most_200_characters() {
    let long = "字".repeat(300);
    assert_eq!(prompt_preview(&long).unwrap().chars().count(), 200);
}

#[test]
fn background_task_notices_are_not_your_words() {
    let notice = "<task-notification>\n<task-id>bv9y8wkxk</task-id>\n<status>completed</status>\n</task-notification>";
    assert_eq!(prompt_preview(notice), None);
    assert_eq!(prompt_preview(&format!("  {notice}")), None);
}
