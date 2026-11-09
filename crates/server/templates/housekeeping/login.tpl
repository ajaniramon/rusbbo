{% set title = locale.hk_acceso %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="login-wrap">
  <div class="login-card">
    <div class="brand">
      <div class="brand-mark">{{ locale.hk_a_2|raw }}</div>
      <div>
        <div class="brand-name">{{ site.siteName }} {{ locale.hk_hotel|raw }}</div>
        <div class="brand-sub">{{ locale.hk_housekeeping_acceso_solo_para_el_staff|raw }}</div>
      </div>
    </div>
    <div class="card">
      <div class="card-body">
        {% include "housekeeping/base/flash.tpl" %}
        <form class="form" method="post" action="{{ hk.path }}/login">
          <div class="field">
            <label for="namefield">{{ locale.hk_usuario|raw }}</label>
            <input type="text" id="namefield" name="hkusername" autocomplete="username" autofocus>
          </div>
          <div class="field">
            <label for="passwordfield">{{ locale.hk_contrasena|raw }}</label>
            <input type="password" id="passwordfield" name="hkpassword" autocomplete="current-password">
          </div>
          <div class="form-actions">
            <button class="btn primary" type="submit">{{ locale.hk_entrar|raw }}</button>
            <a href="/" class="muted small">{{ locale.hk_volver_a_la_web|raw }}</a>
          </div>
        </form>
      </div>
    </div>
    <p class="muted small" style="margin-top:14px">{{ locale.hk_se_requiere_rango_de_moderador_o_superior_los_ac|raw }}</p>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
