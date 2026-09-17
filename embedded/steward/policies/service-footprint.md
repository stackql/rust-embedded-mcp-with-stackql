# Service footprint: {{ DEMO_HOST }}

The service `{{ DEMO_HOST }}` runs on one EC2 instance in AWS region
`{{ AWS_REGION }}` and is reached at `{{ DEMO_HOST }}.{{ DEMO_DOMAIN }}`, an A
record in Cloudflare zone `{{ CLOUDFLARE_ZONE_ID }}`. Check each control
against actual state read live from both providers, not against any state
file. Identify the instance and its security group by the tag
`service = {{ DEMO_HOST }}`.

Reads use the `aws` provider (describe views) and `cloudflare.dns`. Writes
use the `awscc` provider (Cloud Control: INSERT creates, UPDATE patches,
DELETE removes) and `cloudflare.dns.records`.

1. Inventory: exactly one instance and one security group carry the tag
   `service = {{ DEMO_HOST }}` (`aws.ec2.tags` with `region`, `key =
   'service'`, `value = '{{ DEMO_HOST }}'`; `resource_type` is `instance` or
   `security-group`). Then read the instance (`aws.ec2.instances`, `region`
   and `instance_id`) for its state and `public_ip_address`. Ignore
   instances whose state is `terminated` or `shutting-down` (AWS keeps their
   tags readable for a while). If zero or more than one live instance,
   report and stop.

2. Tags: the instance carries `owner`, `cost-centre` and `service`
   (`aws.ec2.tags` with `region` and `resource_id = '<instance id>'`).
   Remediation: `UPDATE awscc.ec2.instances SET PatchDocument =
   string('[{"op":"replace","path":"/Tags","value":[<the full desired tag
   list as {"Key":..,"Value":..}>]}]') WHERE Identifier = '<instance id>' AND
   region = '{{ AWS_REGION }}'`. Keep existing tags; add the missing ones.

3. Ingress: the instance's security group has no inbound rule for port 22
   from `0.0.0.0/0` (`aws.ec2.security_group_rules` with `region`,
   `group_id`, `is_egress = 0`; columns `from_port`, `to_port`, `cidr_ipv_4`,
   `security_group_rule_id`). Ports 443 and 80 from anywhere are expected.
   Remediation: `DELETE FROM awscc.ec2.security_group_ingresses WHERE
   Identifier = '<security_group_rule_id>' AND region = '{{ AWS_REGION }}'`.
   This is a delete; in safe and delete_safe modes the server asks a human
   before running it. If declined, say so and leave it.

4. Edge: an A record `{{ DEMO_HOST }}.{{ DEMO_DOMAIN }}` exists and its
   `content` equals the instance's public IP (`cloudflare.dns.zones_dns_records`
   with `zone_id = '{{ CLOUDFLARE_ZONE_ID }}'`; columns `id`, `name`, `type`,
   `content`, `proxied`, `ttl`).
   Remediation when it is missing: `INSERT INTO cloudflare.dns.zones_dns_records
   (zone_id, name, type, content, ttl, proxied, comment) SELECT
   '{{ CLOUDFLARE_ZONE_ID }}', '{{ DEMO_HOST }}.{{ DEMO_DOMAIN }}', 'A',
   '<public ip read from aws.ec2.instances>', 300, false, 'restored by steward'`.
   When it exists but points elsewhere: re-pointing is a delete of the wrong
   record plus an insert of the right one; propose both, in that order.

5. Dangling edge: no other A record in the zone whose `name` starts with
   `{{ DEMO_HOST }}` points at an IP that is not a public IP of an instance
   in `{{ AWS_REGION }}`. A record like that is a subdomain-takeover risk.
   Remediation: `DELETE FROM cloudflare.dns.records WHERE zone_id =
   '{{ CLOUDFLARE_ZONE_ID }}' AND dns_record_id = '<id>'` (a delete; a
   human is asked).

Never stop, terminate or replace the instance, never delete the security
group or the zone, never change rules for ports 443 or 80.
