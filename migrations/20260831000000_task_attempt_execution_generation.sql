ALTER TABLE task_attempts
    ADD COLUMN execution_generation UUID;

UPDATE task_attempts
SET execution_generation = id
WHERE execution_generation IS NULL;

ALTER TABLE task_attempts
    ALTER COLUMN execution_generation SET NOT NULL;
