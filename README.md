# Stake Delegators API

This Rust application provides a simple API for managing stake delegators. 

## Endpoints

- **GET Endpoint**: The `/get-stake-delegators` endpoint returns information about stake delegators in a JSON format.

Example:
```json
{
    "timestamp": 1703627160,
    "stake_delegators": {
        "frol.near": "qbit.poolv1.near",
        "frolik.near": "qbit.poolv1.near,staked.poolv1.near",
        ...
    }
}
```

- **POST Endpoint**: The `/update-stake-delegators` endpoint allows for the update of stake delegator information.

## Deployment on fly.io

Firstly, you need to create an account and authenticate:

```bash
fly auth
```

Then simply run launch command:

![launch](images/fly-launch.png)

In case you want to redeploy application:

```bash
fly deploy
```

You also can view logs via cli or fly.io dashboard:

```bash
fly logs -a near-stake-delegators-scan
```
The application logs requests and errors using the `pretty_env_logger` crate and provides timestamped logs in a readable format.

The API will be accessible at generated fly.io link.

## Pagoda Console Alerts

You can either call webhook by yourself or use [Pagoda Console](https://console.pagoda.co/) to send POST request to the deployed application if new successful action happens on target *.poolv1.near  
Here's a suggested configuration:

![pagoda console](images/pagoda-console.png)
Feel free to customize and extend the functionality based on your specific use case.
