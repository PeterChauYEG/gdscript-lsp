#!/usr/bin/env bash
set -euo pipefail

job_name="$1"
report_path="$2"

RUST_LABEL_ID="37adccdc-90c5-4bdf-a2ff-43447949dca6"
CI_LABEL_ID="01f4906f-28d8-4ff9-b95c-a1d6de39d1ca"

if [[ -z "${LINEAR_API_KEY:-}" ]]; then
  echo "[file-linear-ticket] WARN: LINEAR_API_KEY not set — skipping" >&2
  exit 0
fi

title="${job_name}: ${GITHUB_REPOSITORY#*/} failing"

report_body="(report unavailable)"
if [[ -f "$report_path" ]]; then
  report_body=$(cat "$report_path")
fi

search_query=$(jq -n --arg title "$title" '{
  query: "query($title: String!) { issues(filter: { team: { key: { eq: \"LAB\" } }, state: { type: { nin: [\"completed\", \"cancelled\"] } }, title: { contains: $title } }, first: 1) { nodes { id identifier url } } }",
  variables: { title: $title }
}')

search_response=$(curl -s -X POST https://api.linear.app/graphql \
  -H "Authorization: $LINEAR_API_KEY" \
  -H "Content-Type: application/json" \
  -d "$search_query")

existing_id=$(echo "$search_response" | jq -r '.data.issues.nodes[0].id // empty')
existing_identifier=$(echo "$search_response" | jq -r '.data.issues.nodes[0].identifier // empty')

if [[ -n "$existing_id" ]]; then
  echo "[file-linear-ticket] existing open ticket $existing_identifier — commenting instead of creating a duplicate"
  comment_body="Still failing on PR #${PR_NUMBER} as of ${RUN_URL}."
  comment_json=$(jq -n --arg issueId "$existing_id" --arg body "$comment_body" \
    '{query:"mutation($issueId:String!,$body:String!){commentCreate(input:{issueId:$issueId,body:$body}){success}}", variables:{issueId:$issueId, body:$body}}')
  curl -s -X POST https://api.linear.app/graphql \
    -H "Authorization: $LINEAR_API_KEY" \
    -H "Content-Type: application/json" \
    -d "$comment_json" > /dev/null
  exit 0
fi

team_response=$(curl -s -X POST https://api.linear.app/graphql \
  -H "Authorization: $LINEAR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"query":"{ teams(filter: { key: { eq: \"LAB\" } }) { nodes { id } } }"}')
team_id=$(echo "$team_response" | jq -r '.data.teams.nodes[0].id // empty')

if [[ -z "$team_id" ]]; then
  echo "[file-linear-ticket] ERROR: could not find LAB team" >&2
  exit 1
fi

states_response=$(curl -s -X POST https://api.linear.app/graphql \
  -H "Authorization: $LINEAR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"query":"{ workflowStates(filter: { team: { key: { eq: \"LAB\" } }, name: { eq: \"Backlog\" } }) { nodes { id } } }"}')
state_id=$(echo "$states_response" | jq -r '.data.workflowStates.nodes[0].id // empty')

if [[ -z "$state_id" ]]; then
  echo "[file-linear-ticket] ERROR: could not find Backlog state" >&2
  exit 1
fi

description="PR: ${PR_URL}
Run: ${RUN_URL}

${report_body}"

variables=$(jq -n \
  --arg teamId "$team_id" \
  --arg title "$title" \
  --arg description "$description" \
  --arg stateId "$state_id" \
  --arg rustLabelId "$RUST_LABEL_ID" \
  --arg ciLabelId "$CI_LABEL_ID" \
  '{
    teamId: $teamId,
    title: $title,
    description: $description,
    stateId: $stateId,
    priority: 3,
    labelIds: [$rustLabelId, $ciLabelId]
  }')

mutation='mutation($teamId: String!, $title: String!, $description: String!, $stateId: String!, $priority: Int, $labelIds: [String!]) {
  issueCreate(input: {
    teamId: $teamId,
    title: $title,
    description: $description,
    stateId: $stateId,
    priority: $priority,
    labelIds: $labelIds
  }) {
    success
    issue { identifier url }
  }
}'

create_response=$(jq -n --arg mutation "$mutation" --argjson variables "$variables" '{query: $mutation, variables: $variables}' | \
  curl -s -X POST https://api.linear.app/graphql \
    -H "Authorization: $LINEAR_API_KEY" \
    -H "Content-Type: application/json" \
    -d @-)

success=$(echo "$create_response" | jq -r '.data.issueCreate.success // false')

if [[ "$success" != "true" ]]; then
  echo "[file-linear-ticket] ERROR: issueCreate failed: $(echo "$create_response" | jq -r '.errors[0].message // "unknown"')" >&2
  exit 1
fi

identifier=$(echo "$create_response" | jq -r '.data.issueCreate.issue.identifier')
url=$(echo "$create_response" | jq -r '.data.issueCreate.issue.url')
echo "[file-linear-ticket] created $identifier: $url"
