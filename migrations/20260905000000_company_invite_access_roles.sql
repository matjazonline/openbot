ALTER TABLE company_invites
ADD COLUMN role TEXT NOT NULL DEFAULT 'member',
ADD CONSTRAINT company_invites_role_check CHECK (role IN ('member', 'admin'));
