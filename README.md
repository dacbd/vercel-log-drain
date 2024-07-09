# vercel-log-drain
A simple log-drain you can deploy to export log messages from Vercel to AWS Cloudwatch.

Feel free to fork and modify to change behavior, trigger other events, or adapt for something other than Cloudwatch

## AWS

### Cloudwatch

The log drain will create new log groups and log streams if they are not present.
Log groups follow this scheme: `/vercel/{project_name}/{vercel_source}`, `project_name` is self-explaining, and `vercel_source` is one of the following `build`, `edge`, `external`, `lambda`, and `static`
The log stream is the vercel deployment ID.

### Permissions

AWS permissions used:
```
logs:DescribeLogGroups
logs:DescribeLogGroups
logs:DescribeLogStreams
logs:CreateLogGroup
logs:CreateLogStream
logs:PutLogEvents
logs:PutRetentionPolicy
```

Terraform example creating a role to be used with the service
```hcl
resource "aws_iam_role" "vercel_log_drain" {
  name               = "vercel-log-drain"
  description        = "Role to be used by the vercel log drain deployment"
  assume_role_policy = data.aws_iam_policy_document.vercel_log_drain_assume.json
}
data "aws_iam_policy_document" "vercel_log_drain_assume" {
    # depends on how you intend to deploy/run the service
}
resource "aws_iam_role_policy" "vercel_log_drain_policy" {
  name   = "vercel-log-drain-policy"
  role   = aws_iam_role.vercel_log_drain.id
  policy = data.aws_iam_policy_document.vercel_log_drain_permissions.json
}
data "aws_iam_policy_document" "vercel_log_drain_permissions" {
  statement {
    actions = [
      "logs:DescribeLogGroups",
      "logs:DescribeLogGroups",
      "logs:DescribeLogStreams",
      "logs:CreateLogGroup",
      "logs:CreateLogStream",
      "logs:PutLogEvents",
      "logs:PutRetentionPolicy",
    ]
    resources = [
      "*"
    ]
  }
}
```

## Operation
As written in my deployment this handled about `~8M` requests per month, with an avg response time (LB -> target) of `1-1.5ms` with an avg memory usage of `~5MB`.
A 3 node deployment (for redundency) with `100m` CPU and `128MB` memory reservations should be able to go quite far.
The response times above are a bit unfair because the system is designed to always responsed to vercel as fast as possible, adding the messages to an internal queue which processes the messages async from the actual POST request which is was receieved from.

If you click Vercel's test log drain button when you are setting up your deployment you may see some messages fail to parse this is because a few of the test messages dont fully follow their documented structure (some fields are missing)

No effort has really been made yet to optimize the code, still it is performant enough to handle anything, but feel free to contribute optimizations or idiomatic code corrections, I wrote this in a vacuum.

### JSON logging in vercel
If you have structured JSON logging ie the contents of `messaage` is a json string, the service attempts to parse it as json so a fully JSON message can be pass downstream, vs a string containing json. 

Example: `{ "message": { "method": "GET" } }` vs `{ "message": "{ \"method\": \"GET\" }" }`

This helps with log queries in cloudwatch or if modified your downsteam system to search or filter on data not just provided by vercel but also your own JSON logging in the deployed application.

## Configure

- logging level (default: `INFO`)
    - via env: `export VERCEL_LOG_DRAIN_LOG_LEVEL="DEBUG"`
    - via cli arg: `vercel-log-drain -l DEBUG`
- listening port (default: `8000`)
    - via env: `export VERLCEL_LOG_DRAIN_PORT=3000`
    - via cli arg: `vercel-log-drain -p 3000`
- listening interface (default: `0.0.0.0`)
    - via env: `export VERLCEL_LOG_DRAIN_IP="127.0.0.1"`
    - via cli arg: `vercel-log-drain -i 127.0.0.1`
- Vercel's Verify response headed (NO default)
    - via env: `export VERCEL_VERIFY="deadbeef"`
    - via cli arg: `vercel-log-drain --vercel-verify deadbeef`
- vercel's Secert (NO default)
    - via env: `export VERCEL_SECRET="deadbeef"`
    - via cli arg: `vercel-log-drain --vercel-SECRET deadbeef`

## Related vercel documentation

https://vercel.com/docs/observability/log-drains-overview/log-drains-reference#json-log-drains
https://vercel.com/docs/observability/log-drains-overview/log-drains-reference#secure-log-drains

