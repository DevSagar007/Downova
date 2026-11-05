-- CORE-08 (ADR 0025): one optional schedule per queue, and its durable evaluation state.
--   schedule_json          the queue's schedule (JSON, validated by the service); NULL: none.
--   schedule_evaluated_ms  when the service last evaluated it (missed-window detection).
--   catch_up_for_ms        start instant of the missed window last caught up (its identity):
--                          the same occurrence is never caught up twice, across restarts.
--   catch_up_from_ms / catch_up_until_ms
--                          the open catch-up window, recorded before it takes effect; absolute
--                          instants, so a restart cannot extend it, and a clock set back before
--                          its start ends it.
-- Editing or removing a schedule clears the catch-up columns; deleting the queue deletes all.
ALTER TABLE queues ADD COLUMN schedule_json TEXT;
ALTER TABLE queues ADD COLUMN schedule_evaluated_ms INTEGER;
ALTER TABLE queues ADD COLUMN catch_up_for_ms INTEGER;
ALTER TABLE queues ADD COLUMN catch_up_from_ms INTEGER;
ALTER TABLE queues ADD COLUMN catch_up_until_ms INTEGER;
