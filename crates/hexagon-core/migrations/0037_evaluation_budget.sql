-- D08: parent reservations include their child request holds, never double count.
CREATE TABLE evaluation_budget_rounds (
 id TEXT PRIMARY KEY CHECK(id IN ('scripted_debug','paid_first_round')),
 total_mc INTEGER NOT NULL CHECK(total_mc>0 AND total_mc<=20000000),
 pilot_mc INTEGER NOT NULL CHECK(pilot_mc>0 AND pilot_mc<=2000000),
 blocked INTEGER NOT NULL DEFAULT 0 CHECK(blocked IN (0,1))
);
CREATE TABLE evaluation_budget_plans (
 plan_id TEXT PRIMARY KEY REFERENCES evaluation_plans(id),
 price_json TEXT NOT NULL CHECK(json_valid(price_json))
);
CREATE TABLE evaluation_budget_pairs (
 id TEXT PRIMARY KEY, round_id TEXT NOT NULL REFERENCES evaluation_budget_rounds(id),
 host TEXT NOT NULL, plan_id TEXT NOT NULL, pilot INTEGER NOT NULL CHECK(pilot IN (0,1)),
 allowance_mc INTEGER NOT NULL CHECK(allowance_mc>0)
);
CREATE TABLE evaluation_budget_runs (
 id TEXT PRIMARY KEY, pair_id TEXT NOT NULL REFERENCES evaluation_budget_pairs(id),
 position INTEGER NOT NULL, run_id TEXT, workspace TEXT,
 allowance_mc INTEGER NOT NULL CHECK(allowance_mc>0 AND allowance_mc<=500000),
 request_limit INTEGER NOT NULL CHECK(request_limit>0 AND request_limit<=80),
 price_json TEXT NOT NULL CHECK(json_valid(price_json)),
 closed INTEGER NOT NULL DEFAULT 0 CHECK(closed IN (0,1)),
 UNIQUE(pair_id,position)
);
CREATE TABLE evaluation_budget_requests (
 id TEXT PRIMARY KEY, run_key TEXT NOT NULL REFERENCES evaluation_budget_runs(id),
 local_request_id TEXT NOT NULL, purpose TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('pending','known','unknown','not_sent')),
 dispatch_started INTEGER NOT NULL DEFAULT 0 CHECK(dispatch_started IN (0,1)),
 confirmed INTEGER NOT NULL DEFAULT 0 CHECK(confirmed IN (0,1)),
 bound_mc INTEGER NOT NULL CHECK(bound_mc>=0),
 known_mc INTEGER NOT NULL DEFAULT 0 CHECK(known_mc>=0),
 held_mc INTEGER NOT NULL CHECK(held_mc>=0),
 receipt_digest TEXT,
 UNIQUE(run_key,local_request_id)
);

-- Append-only supplier facts: conflicting/overflow receipts remain explainable.
CREATE TABLE evaluation_budget_receipts (
 request_id TEXT NOT NULL REFERENCES evaluation_budget_requests(id),
 digest TEXT NOT NULL, receipt_json TEXT NOT NULL CHECK(json_valid(receipt_json)),
 cost_text TEXT, conflict INTEGER NOT NULL CHECK(conflict IN (0,1)),
 PRIMARY KEY(request_id,digest)
);
