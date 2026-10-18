-- A28: whether a part shared with everyone shows its picture on the sign-in wall. On by default, as the owner asked;
-- its owner turns it off on the part page. `instance_setting` `sign_in_wall` turns the whole wall off.
ALTER TABLE part ADD COLUMN on_wall boolean NOT NULL DEFAULT true;
