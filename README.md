# vercel-log-drain

A simple log-drain you can deploy to export log messages from Vercel to one or more sources!

## Drivers

### AWS Cloudwatch

> *Available with the `cloudwatch` [feature](#cargo-features) (enabled by default).*

To use the CloudWatch driver, you'll need to either:

- add a environment variable for `VERCEL_LOG_DRAIN_CLOUDWATCH_ENABLED=true`
- add the `--cloudwatch-enabled` cli flag

The log drain will create new log groups and log streams if they are not present.
Log groups follow this scheme: `/vercel/{project_name}/{vercel_source}`, `project_name` is self-explaining, and `vercel_source` is one of the following `build`, `edge`, `external`, `lambda`, and `static`
The log stream is the vercel deployment ID.

#### Permissions

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

### [Grafana Loki](https://grafana.com/docs/loki/latest/)

> *Available with the `loki` [feature](#cargo-features) (enabled by default).*

To use the loki driver, you'll need to set up:

- `--loki-enabled` (or the env var `VERCEL_LOG_DRAIN_LOKI_ENABLED=true`)
- `--loki-url` (or the env var `VERCEL_LOG_DRAIN_LOKI_URL`)
- (optional, if you have basic auth) `--loki-basic-auth-user` and `--loki-basic-auth-pass` (or the corresponding env vars `VERCEL_LOG_DRAIN_LOKI_USER` and `VERCEL_LOG_DRAIN_LOKI_PASS`)

## Configuration

| CLI Flag                 | Environment Variable                 | Default Value | Description                              |
| ------------------------ | ------------------------------------ | ------------- | ---------------------------------------- |
| `-l, --log`              | `VERCEL_LOG_DRAIN_LOG_LEVEL`         | `INFO`        | Log level                                |
| `-i, --ip`               | `VERCEL_LOG_DRAIN_IP`                | `"0.0.0.0"`   | IP address to bind to                    |
| `-p, --port`             | `VERCEL_LOG_DRAIN_PORT`              | `8000`        | Port number                              |
| `--vercel-verify`        | `VERCEL_VERIFY`                      | -             | Vercel verification token                |
| `--vercel-secret`        | `VERCEL_SECRET`                      | -             | Vercel secret                            |
| `--enable-metrics`       | `VERCEL_LOG_DRAIN_ENABLE_METRICS`    | -             | Enable prometheus metrics endpoint       |
| `--metrics-prefix`       | `VERCEL_LOG_DRAIN_METRICS_PREFIX`    | "drain"       | the shared prefix to use for all metrics |
| `--enable-cloudwatch`    | `VERCEL_LOG_DRAIN_ENABLE_CLOUDWATCH` | -             | Enable CloudWatch integration            |
| `--enable-loki`          | `VERCEL_LOG_DRAIN_ENABLE_LOKI`       | -             | Enable Loki integration                  |
| `--loki-url`             | `VERCEL_LOG_DRAIN_LOKI_URL`          | `""`          | Loki URL                                 |
| `--loki-basic-auth-user` | `VERCEL_LOG_DRAIN_LOKI_USER`         | `""`          | Loki basic auth username                 |
| `--loki-basic-auth-pass` | `VERCEL_LOG_DRAIN_LOKI_PASS`         | `""`          | Loki basic auth password                 |

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

## Cargo features

`cargo` will build `vercel-log-drain` with **all**
[features](https://doc.rust-lang.org/cargo/reference/features.html) by default:

Feature      | Description
------------ | --------
`cloudwatch` | [AWS Cloudwatch](#aws-cloudwatch) driver
`loki`       | [Grafana Loki](#grafana-loki) driver

If you want a smaller binary, you could disable all of them with
`--no-default-features`, and then only re-enable the features you use.

For example, to build `vercel-log-drain` with only AWS Cloudwatch support:

```sh
cargo build --release --no-default-features --features cloudwatch
```

This can also be used when building the Docker image:

```sh
docker build -t vercel-log-drain --build-arg 'BUILD_ARGS=--no-default-features --features cloudwatch' .
```

## Testing

```bash
cargo build

# run the server
./target/debug/vercel-log-drain --enable-metrics --vercel-secret "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef" --vercel-verify verify --log DEBUG
```

If you're using the nix environment, there are some helpful scripts for running and sending test payloads to the server!

```bash
# build
cargo build

# run the server
run  # you'll need to add env vars or your options here! example (i have an http sink server running on :8080 that is logging all requests incoming):
run --enable-loki --loki-url http://localhost:8080/ingest

# send test payloads
test_drain ./src/fixtures/sample_1.json
```

## Related vercel documentation

- [Vercel JSON Log Drains](https://vercel.com/docs/observability/log-drains-overview/log-drains-reference#json-log-drains)
- [Vercel Secure Log Drains](https://vercel.com/docs/observability/log-drains-overview/log-drains-reference#secure-log-drains)
