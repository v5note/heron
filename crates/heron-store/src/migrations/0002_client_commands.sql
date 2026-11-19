-- Client scan locations now include legacy slash-command directories (decision D9).
ALTER TABLE client ADD COLUMN global_commands TEXT NOT NULL DEFAULT '[]';
ALTER TABLE client ADD COLUMN project_commands TEXT NOT NULL DEFAULT '[]';
