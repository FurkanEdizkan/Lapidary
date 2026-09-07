-- How a picture is framed, kept beside the picture rather than baked into it.
--
-- The owner asked to "adjust the image view when ingested", and the plan settled what that
-- means: **the framing is data, applied at display time.** The normalized WebP is written
-- once and never re-encoded, so re-framing costs nothing, can be undone, and never stacks a
-- second generation of lossless-over-lossy artefacts onto a photograph somebody is using to
-- decide something. Baking a crop means a re-upload every time you change your mind.
--
-- **Three columns and not a crop rectangle**, because these three are exactly CSS's
-- `object-fit` and `object-position` and the browser already does the work. A free-form crop
-- box would be a second geometry to keep in step with the one the browser applies anyway,
-- for a picture on a card that is a few hundred pixels across.

-- `cover` fills the frame and loses the edges; `contain` shows the whole picture and letters
-- box it. `cover` is the default because a gallery of same-sized tiles is what a person
-- scanning parts wants, and a photograph with its edges cropped still reads as the part.
--
-- Text and not an enum, matching every other discriminator in this schema: adding a value
-- must not need a migration. The CHECK is what keeps the set closed in the meantime.
alter table part_image add column fit text not null default 'cover';

alter table part_image add constraint part_image_known_fit
  check (fit in ('cover', 'contain'));

-- Where in the picture to keep when `cover` crops it, as a fraction of each edge: 0 is the
-- left or top, 1 the right or bottom, and 0.5 — the default — is the centre, which is what
-- `object-position` does when nobody says otherwise.
--
-- `double precision` rather than an integer percentage: this is a fraction of an edge and
-- not a measurement, so `CLAUDE.md`'s rule about money in floats does not apply and rounding
-- a focal point to a whole percent would be an arbitrary limit on a control that is dragged.
alter table part_image add column focus_x double precision not null default 0.5;
alter table part_image add column focus_y double precision not null default 0.5;

-- In range, because a value outside it is not a framing anybody chose — it is a bug in
-- whatever wrote it, and the useful place to find that out is the insert rather than a
-- picture that renders somewhere off screen.
alter table part_image add constraint part_image_focus_in_range
  check (focus_x between 0 and 1 and focus_y between 0 and 1);
