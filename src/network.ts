import { fetch } from '@tauri-apps/plugin-http';

export enum NetworkState {
  Online,
  BjutCampus, // Has campus network but not logged in
  Offline
}

export enum LoginType {
  Type1_221_98,
  Type2_251_3,
  Type3_172_30,
  Unknown
}

export async function checkInternet(): Promise<boolean> {
  try {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 1500);
    const response = await fetch('http://captive.apple.com/hotspot-detect.html', {
      method: 'GET',
      signal: controller.signal,
      headers: {
        'Cache-Control': 'no-cache'
      }
    });
    clearTimeout(timeoutId);
    
    if (response.ok) {
      const text = await response.text();
      if (text.includes('Success')) {
        return true;
      }
    }
    return false;
  } catch (error) {
    return false;
  }
}

export async function detectLoginType(): Promise<LoginType> {
  const ips = [
    { ip: '10.21.221.98', type: LoginType.Type1_221_98, url: 'http://10.21.221.98/' },
    { ip: '10.21.251.3', type: LoginType.Type2_251_3, url: 'http://10.21.251.3/' },
    { ip: '172.30.201.2', type: LoginType.Type3_172_30, url: 'http://172.30.201.2/' },
    { ip: '172.30.201.10', type: LoginType.Type3_172_30, url: 'http://172.30.201.10/' }
  ];

  const checkTarget = async (target: typeof ips[0]): Promise<LoginType> => {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 1500);
    try {
      const response = await fetch(target.url, {
        method: 'GET',
        signal: controller.signal,
        headers: { 'Cache-Control': 'no-cache' }
      });
      clearTimeout(timeoutId);
      if (response.status !== 0) {
        return target.type;
      }
    } catch (e) {
      clearTimeout(timeoutId);
    }
    throw new Error('Not match');
  };

  try {
    return await Promise.any(ips.map(checkTarget));
  } catch (err) {
    return LoginType.Unknown;
  }
}

export async function loginToCampusNetwork(type: LoginType, user: string, pass: string): Promise<{ success: boolean, msg: string }> {
  try {
    if (type === LoginType.Type1_221_98) {
      const v = Math.floor(Math.random() * 10000).toString().padStart(4, '0');
      const url = `http://10.21.221.98:801/eportal/portal/login?callback=dr1003&login_method=1&user_account=${encodeURIComponent(user + '@campus')}&user_password=${encodeURIComponent(pass)}&wlan_user_ip=&wlan_user_ipv6=&wlan_user_mac=000000000000&wlan_ac_ip=&wlan_ac_name=&jsVersion=4.2.1&terminal_type=1&lang=zh-cn&v=${v}`;
      
      const response = await fetch(url, { method: 'GET' });
      const text = await response.text();
      return parseDr1003Response(text);
    } 
    else if (type === LoginType.Type2_251_3) {
      const v = Math.floor(Math.random() * 10000).toString().padStart(4, '0');
      const url = `http://10.21.251.3/drcom/login?callback=dr1002&DDDDD=${encodeURIComponent(user)}&upass=${encodeURIComponent(pass)}&0MKKey=123456&R1=0&R2=&R3=0&R6=0&para=00&v6ip=&terminal_type=1&lang=zh-cn&jsVersion=4.1&v=${v}`;
      
      const response = await fetch(url, { method: 'GET' });
      const text = await response.text();
      return parseDr1003Response(text);
    }
    else if (type === LoginType.Type3_172_30) {
      // Step 1: Get IPv6
      const body1 = new URLSearchParams();
      body1.append('DDDDD', user);
      body1.append('upass', pass);
      body1.append('v46s', '0');
      body1.append('0MKKey', '');
      
      const res1 = await fetch('https://lgn6.bjut.edu.cn/V6?https://lgn.bjut.edu.cn', {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: body1.toString()
      });
      const html = await res1.text();
      
      let v6ip = '';
      const v6ipMatch = html.match(/<input[^>]*name="v6ip"[^>]*value="([^"]*)"/i);
      if (v6ipMatch && v6ipMatch[1]) {
        v6ip = v6ipMatch[1];
      }

      // Step 2: Login
      const body2 = new URLSearchParams();
      body2.append('DDDDD', user);
      body2.append('upass', pass);
      body2.append('0MKKey', 'Login');
      body2.append('v6ip', v6ip);
      
      const res2 = await fetch('https://lgn.bjut.edu.cn', {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: body2.toString()
      });
      const finalHtml = await res2.text();
      
      if (finalHtml.includes('DispQianFei') || finalHtml.includes('Msg=')) {
         return { success: false, msg: '登录失败，请检查账号密码或余额' };
      }
      return { success: true, msg: 'Portal协议认证成功！' };
    }
    
    return { success: false, msg: '未知的登录类型' };
  } catch (err: any) {
    const errorMsg = err instanceof Error ? err.message : (typeof err === 'string' ? err : JSON.stringify(err));
    return { success: false, msg: `请求出错: ${errorMsg}` };
  }
}

function parseDr1003Response(text: string): { success: boolean, msg: string } {
  try {
    const match = text.match(/dr100\d\((.*)\)/);
    if (match && match[1]) {
      const data = JSON.parse(match[1]);
      if (data.result === 1) {
        return { success: true, msg: 'Portal协议认证成功！' };
      } else {
        return { success: false, msg: data.msg || data.msga || '未知错误' };
      }
    }
    return { success: false, msg: '无效的响应格式' };
  } catch (e) {
    return { success: false, msg: '解析响应失败' };
  }
}

export async function fetchUserInfo(): Promise<{ account: string, balance: string, flow: string } | null> {
  try {
    const v = Math.floor(Math.random() * 10000).toString().padStart(4, '0');
    const url = `http://172.30.201.2:801/eportal/portal/page/loadUserInfo?callback=726427262624&lang=6c7e3b7578&program_index=79225954737327212323222f212e2723&page_index=755e577b7c4e27212323222f212e2320&user_account=&wlan_user_ip=&wlan_user_ipv6=&wlan_user_mac=262626262626262626262626&jsVersion=22384e&encrypt=1&v=${v}&lang=zh`;
    
    const response = await fetch(url, { method: 'GET' });
    const text = await response.text();
    const match = text.match(/dr100\d\((.*)\)/);
    
    if (match && match[1]) {
      const data = JSON.parse(match[1]);
      if (data.user_info) {
        const info = data.user_info;
        const packageName = info.package_group_name || '';
        let totalFlow = 30; // default 30GB
        if (packageName.includes('Test')) totalFlow = 999999;
        else if (packageName.includes('10元')) totalFlow = 60;
        else if (packageName.includes('20元')) totalFlow = 120;
        else if (packageName.includes('30元')) totalFlow = 180;
        else if (packageName.includes('60元')) totalFlow = 400;

        let useFlowStr = info.use_flow || '0GB';
        let useFlow = parseFloat(useFlowStr.replace(/[^\d.]/g, ''));
        if (useFlowStr.includes('MB')) useFlow = useFlow / 1024;
        
        let remaining = totalFlow - useFlow;
        if (totalFlow === 999999) remaining = 999999;
        
        return {
          account: info.account,
          balance: info.balance,
          flow: totalFlow === 999999 ? '无限' : `${remaining.toFixed(2)} GB`
        };
      }
    }
  } catch (err) {
    // silently fail
  }
  return null;
}
