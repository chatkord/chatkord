# Sourced by the package scripts. `within_budget BINARY SYSTEM` holds the binary, stripped as its package installs it,
# to the most bytes scripts/size-budget.txt lets SYSTEM's take, and ends the script when it is over or has none.
within_budget() {
  local size budget
  size=$(stat -c %s "$1")
  budget=$(awk -v os="$2" '$1 == os { print $2 }' scripts/size-budget.txt)
  [[ "$budget" =~ ^[0-9]+$ ]] || { echo "FAIL: scripts/size-budget.txt names no budget for $2"; exit 1; }
  echo "chatkord on $2: $size bytes, budget $budget"
  (( size <= budget )) ||
    { echo "FAIL: the binary is $((size - budget)) bytes over the budget for $2 in scripts/size-budget.txt"; exit 1; }
}
