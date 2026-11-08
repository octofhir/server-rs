-- One projection shared with the native UPS command's immutable input binding.
CREATE OR REPLACE FUNCTION octofhir_dicom_task_projection(resource jsonb)
RETURNS jsonb LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
 SELECT jsonb_build_object('id',resource->'id','for',resource->'for',
  'focus',resource->'focus','code',resource->'code','input',resource->'input',
  'priority',resource->'priority','restriction',resource->'restriction',
  'requestedPeriod',resource->'requestedPeriod','doNotPerform',resource->'doNotPerform',
  '_doNotPerform',resource->'_doNotPerform','modifierExtension',resource->'modifierExtension',
  'owner',resource->'owner','performerType',resource->'performerType',
  'requestedPerformer',resource->'requestedPerformer','location',resource->'location','intent',resource->'intent');
$$;
CREATE OR REPLACE FUNCTION octofhir_dicom_task_guard()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE binding record; target_id text;
BEGIN
 IF to_regclass('dicom_ups_tasks') IS NULL THEN
  IF TG_OP='DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF;
 END IF;
 IF TG_OP='DELETE' THEN target_id:=OLD.id; ELSE target_id:=NEW.id; END IF;
 SELECT request_metadata,remote_state,desired_revision,applied_revision,pending_job,cancelled_before_submission
  INTO binding FROM dicom_ups_tasks WHERE task_id=target_id;
 IF NOT FOUND THEN
  IF TG_OP='DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF;
 END IF;
 IF TG_OP='DELETE' THEN
  IF (NOT binding.cancelled_before_submission AND (binding.remote_state IS NULL OR binding.remote_state NOT IN ('COMPLETED','CANCELED')))
   OR binding.desired_revision>binding.applied_revision OR binding.pending_job IS NOT NULL THEN
   RAISE EXCEPTION USING ERRCODE='23514',CONSTRAINT='dicom_ups_task_binding',
    MESSAGE='Cancel and reconcile the active UPS workitem before deleting its Task';
  END IF;
  RETURN OLD;
 END IF;
 IF octofhir_dicom_task_projection(NEW.resource) IS DISTINCT FROM binding.request_metadata->'task_projection' THEN
  RAISE EXCEPTION USING ERRCODE='23514',CONSTRAINT='dicom_ups_task_binding',
   MESSAGE='Managed UPS Task inputs are immutable; create a new Task for different inputs';
 END IF;
 RETURN NEW;
END $$;
