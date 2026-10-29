#!/usr/bin/env bash
# Moves issues and pull requests on the project board as they change. Run by
# .github/workflows/project.yml with these set:
#
#   GH_TOKEN        a token that can edit the project (the PROJECT_TOKEN secret)
#   PROJECT_OWNER   the user who owns the project, and PROJECT_NUMBER its number
#   EVENT, ACTION   the event name and its action, such as issues and closed
#   NODE_ID         the issue's or pull request's GraphQL ID
#   DRAFT, MERGED   for pull requests: "true" or "false"
#   STATE_REASON    for closed issues: completed or not_planned
#
# Issues start in Backlog. Opening a pull request moves it and the issues it closes to In
# review (In progress while it is a draft); merging moves them to Done. An issue closed as
# completed moves to Done. Ready and In progress are otherwise set by hand.
set -euo pipefail

project=$(gh api graphql -F owner="$PROJECT_OWNER" -F number="$PROJECT_NUMBER" -f query='
  query($owner: String!, $number: Int!) {
    user(login: $owner) {
      projectV2(number: $number) {
        id
        field(name: "Status") { ... on ProjectV2SingleSelectField { id options { id name } } }
      }
    }
  }')
project_id=$(jq -r '.data.user.projectV2.id' <<<"$project")
field_id=$(jq -r '.data.user.projectV2.field.id' <<<"$project")

# move CONTENT_ID STATUS: adds the issue or pull request to the board if needed, then sets
# its status.
move() {
  local option item
  option=$(jq -r --arg name "$2" '.data.user.projectV2.field.options[] | select(.name == $name) | .id' <<<"$project")
  item=$(gh api graphql -f project="$project_id" -f content="$1" --jq '.data.addProjectV2ItemById.item.id' -f query='
    mutation($project: ID!, $content: ID!) {
      addProjectV2ItemById(input: {projectId: $project, contentId: $content}) { item { id } }
    }')
  gh api graphql -f project="$project_id" -f item="$item" -f field="$field_id" -f option="$option" -f query='
    mutation($project: ID!, $item: ID!, $field: ID!, $option: String!) {
      updateProjectV2ItemFieldValue(input: {
        projectId: $project, itemId: $item, fieldId: $field,
        value: {singleSelectOptionId: $option}
      }) { projectV2Item { id } }
    }' >/dev/null
  echo "$2: $1"
}

# move_with_issues PR_ID STATUS: moves a pull request and every issue it closes.
move_with_issues() {
  move "$1" "$2"
  local issue
  for issue in $(gh api graphql -f id="$1" --jq '.data.node.closingIssuesReferences.nodes[].id' -f query='
    query($id: ID!) {
      node(id: $id) { ... on PullRequest { closingIssuesReferences(first: 20) { nodes { id } } } }
    }'); do
    move "$issue" "$2"
  done
}

case "$EVENT:$ACTION" in
  issues:opened | issues:reopened)
    move "$NODE_ID" Backlog
    ;;
  issues:closed)
    if [ "${STATE_REASON:-}" = completed ]; then move "$NODE_ID" Done; fi
    ;;
  pull_request_target:opened | pull_request_target:reopened | pull_request_target:ready_for_review)
    if [ "$DRAFT" = true ]; then
      move_with_issues "$NODE_ID" "In progress"
    else
      move_with_issues "$NODE_ID" "In review"
    fi
    ;;
  pull_request_target:converted_to_draft)
    move_with_issues "$NODE_ID" "In progress"
    ;;
  pull_request_target:closed)
    if [ "$MERGED" = true ]; then move_with_issues "$NODE_ID" Done; fi
    ;;
esac
